use bytemuck::Zeroable;
use parking_lot::{MappedMutexGuard, Mutex};
use std::ops::Deref;
use std::sync::Arc;

use crate::frame_info::{GameState, PlayerInput};
use crate::input_queue::InputQueue;
use crate::network::messages::ConnectionStatus;
use crate::{Config, Frame, GgrsRequest, InputStatus, PlayerHandle, NULL_FRAME};

/// An [`Arc<Mutex>`] that you can [`save()`]/[`load()`] a `T` to/from. These will be handed to the user as part of a [`GgrsRequest`].
///
/// [`save()`]: GameStateCell#method.save
/// [`load()`]: GameStateCell#method.load
pub struct GameStateCell<T>(Arc<Mutex<GameState<T>>>);

impl<T> GameStateCell<T> {
    /// Saves a `T` the user creates into the cell.
    pub fn save(&self, frame: Frame, data: Option<T>, checksum: Option<u128>) {
        let mut state = self.0.lock();
        assert!(frame != NULL_FRAME);
        state.frame = frame;
        state.data = data;
        state.checksum = checksum;
    }

    /// Provides direct access to the `T` that the user previously saved into the cell (if there was
    /// one previously saved), without cloning it.
    ///
    /// You probably want to use [load()](Self::load) instead to clone the data; this function is
    /// useful only in niche use cases.
    ///
    /// # Example usage
    ///
    /// ```
    /// # use ggrs::{Frame, GameStateCell};
    /// // Setup normally performed by GGRS behind the scenes
    /// let mut cell = GameStateCell::<MyGameState>::default();
    /// let frame_num: Frame = 0;
    ///
    /// // The state of our example game will be just a String, and our game state isn't Clone
    /// struct MyGameState { player_name: String };
    ///
    /// // Setup you do when GGRS requests you to save game state
    /// {
    ///     let game_state = MyGameState { player_name: "alex".to_owned() };
    ///     let checksum = None;
    ///     // (in real usage, save a checksum! We omit it here because it's not
    ///     // relevant to this example)
    ///     cell.save(frame_num, Some(game_state), checksum);
    /// }
    ///
    /// // We can't use load() to access the game state, because it's not Clone
    /// // println!("{}", cell.load().player_name); // compile error: Clone bound not satisfied
    ///
    /// // But we can still read the game state without cloning:
    /// let game_state_accessor = cell.data().expect("should have a gamestate stored");
    /// assert_eq!(game_state_accessor.player_name, "alex");
    /// ```
    ///
    /// If you really, really need mutable access to the `T`, then consider using the aptly named
    /// [GameStateAccessor::as_mut_dangerous()].
    pub fn data(&self) -> Option<GameStateAccessor<'_, T>> {
        if let Ok(mapped_data) =
            parking_lot::MutexGuard::try_map(self.0.lock(), |state| state.data.as_mut())
        {
            return Some(GameStateAccessor(mapped_data));
        } else {
            None
        }
    }

    pub(crate) fn frame(&self) -> Frame {
        self.0.lock().frame
    }

    pub(crate) fn checksum(&self) -> Option<u128> {
        self.0.lock().checksum
    }
}

impl<T: Clone> GameStateCell<T> {
    /// Loads a `T` that the user previously saved into this cell, by cloning the `T`.
    ///
    /// See also [data()](Self::data) if you want a reference to the `T` without cloning it.
    pub fn load(&self) -> Option<T> {
        let data = self.data()?;
        Some(data.clone())
    }
}

impl<T> Default for GameStateCell<T> {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(GameState::default())))
    }
}

impl<T> Clone for GameStateCell<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> std::fmt::Debug for GameStateCell<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.0.lock();
        f.debug_struct("GameStateCell")
            .field("frame", &inner.frame)
            .field("checksum", &inner.checksum)
            .finish_non_exhaustive()
    }
}

/// A read-only accessor for the `T` that the user previously saved into a [GameStateCell].
///
/// You can use [deref()](Deref::deref) to access the `T` without cloning it; see
/// [GameStateCell::data()](GameStateCell::data) for a usage example.
///
/// This type exists to A) hide the type of the lock guard that allows thread-safe access to the
///  saved `T` so that it does not form part of GGRS API and B) make dangerous mutable access to the
///  `T` very explicit (see [as_mut_dangerous()](Self::as_mut_dangerous)).
pub struct GameStateAccessor<'c, T>(MappedMutexGuard<'c, T>);

impl<'c, T> Deref for GameStateAccessor<'c, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'c, T> GameStateAccessor<'c, T> {
    /// Get mutable access to the `T` that the user previously saved into a [GameStateCell].
    ///
    /// You probably do not need this! It's safer to use [Self::deref()](Deref::deref) instead;
    /// see [GameStateCell::data()](GameStateCell::data) for a usage example.
    ///
    /// **Danger**: the underlying `T` must _not_ be modified in any way that affects (or may ever
    /// in future affect) game logic. If this invariant is violated, you will almost certainly get
    /// desyncs.
    pub fn as_mut_dangerous(&mut self) -> &mut T {
        &mut self.0
    }
}

pub(crate) struct SavedStates<T> {
    pub states: Vec<GameStateCell<T>>,
    max_pred: usize,
}

impl<T> SavedStates<T> {
    fn new(max_pred: usize) -> Self {
        let mut states = Vec::with_capacity(max_pred);
        for _ in 0..max_pred {
            states.push(GameStateCell::default());
        }

        // if lockstep, we still provide a single cell for saving.
        if max_pred == 0 {
            states.push(GameStateCell::default());
        }

        Self { states, max_pred }
    }

    fn get_cell(&self, frame: Frame) -> GameStateCell<T> {
        // if lockstep, we still provide a single cell for saving.
        if self.max_pred == 0 {
            return self.states[0].clone();
        }
        assert!(frame >= 0);
        let pos = frame as usize % self.states.len();
        self.states[pos].clone()
    }
}

pub(crate) struct SyncLayer<T>
where
    T: Config,
{
    num_players: usize,
    max_prediction: usize,
    saved_states: SavedStates<T::State>,
    last_confirmed_frame: Frame,
    last_saved_frame: Frame,
    current_frame: Frame,
    input_queues: Vec<InputQueue<T>>,
}

impl<T: Config> SyncLayer<T> {
    /// Creates a new `SyncLayer` instance with given values.
    pub(crate) fn new(num_players: usize, max_prediction: usize) -> Self {
        // initialize input_queues
        let mut input_queues = Vec::new();
        for _ in 0..num_players {
            input_queues.push(InputQueue::new());
        }
        Self {
            num_players,
            max_prediction,
            last_confirmed_frame: NULL_FRAME,
            last_saved_frame: NULL_FRAME,
            current_frame: 0,
            saved_states: SavedStates::new(max_prediction),
            input_queues,
        }
    }

    pub(crate) fn current_frame(&self) -> Frame {
        self.current_frame
    }

    pub(crate) fn advance_frame(&mut self) {
        self.current_frame += 1;
    }

    pub(crate) fn save_current_state(&mut self) -> GgrsRequest<T> {
        self.last_saved_frame = self.current_frame;
        let cell = self.saved_states.get_cell(self.current_frame);
        GgrsRequest::SaveGameState {
            cell,
            frame: self.current_frame,
        }
    }

    pub(crate) fn set_frame_delay(&mut self, player_handle: PlayerHandle, delay: usize) {
        assert!(player_handle < self.num_players as PlayerHandle);
        self.input_queues[player_handle].set_frame_delay(delay);
    }

    pub(crate) fn reset_prediction(&mut self) {
        for i in 0..self.num_players {
            self.input_queues[i].reset_prediction();
        }
    }

    /// Loads the gamestate indicated by `frame_to_load`.
    pub(crate) fn load_frame(&mut self, frame_to_load: Frame) -> GgrsRequest<T> {
        // The state should not be the current state or the state should not be in the future or too far away in the past
        assert!(
            frame_to_load != NULL_FRAME
                && frame_to_load < self.current_frame
                && frame_to_load >= self.current_frame - self.max_prediction as i32
        );

        let cell = self.saved_states.get_cell(frame_to_load);
        assert_eq!(cell.0.lock().frame, frame_to_load);
        self.current_frame = frame_to_load;

        GgrsRequest::LoadGameState {
            cell,
            frame: frame_to_load,
        }
    }

    /// Adds local input to the corresponding input queue. Checks if the prediction threshold has been reached. Returns the frame number where the input is actually added to.
    /// This number will only be different if the input delay was set to a number higher than 0.
    pub(crate) fn add_local_input(
        &mut self,
        player_handle: PlayerHandle,
        input: PlayerInput<T::Input>,
    ) -> Frame {
        // The input provided should match the current frame, we account for input delay later
        assert_eq!(input.frame, self.current_frame);
        self.input_queues[player_handle].add_input(input)
    }

    /// Adds remote input to the corresponding input queue.
    /// Unlike `add_local_input`, this will not check for correct conditions, as remote inputs have already been checked on another device.
    pub(crate) fn add_remote_input(
        &mut self,
        player_handle: PlayerHandle,
        input: PlayerInput<T::Input>,
    ) {
        self.input_queues[player_handle].add_input(input);
    }

    /// Returns inputs for all players for the current frame of the sync layer. If there are none for a specific player, return predictions.
    pub(crate) fn synchronized_inputs(
        &mut self,
        connect_status: &[ConnectionStatus],
    ) -> Vec<(T::Input, InputStatus)> {
        let mut inputs = Vec::new();
        for (i, con_stat) in connect_status.iter().enumerate() {
            if con_stat.disconnected && con_stat.last_frame < self.current_frame {
                inputs.push((T::Input::zeroed(), InputStatus::Disconnected));
            } else {
                inputs.push(self.input_queues[i].input(self.current_frame));
            }
        }
        inputs
    }

    /// Returns confirmed inputs for all players for the current frame of the sync layer.
    pub(crate) fn confirmed_inputs(
        &self,
        frame: Frame,
        connect_status: &[ConnectionStatus],
    ) -> Vec<PlayerInput<T::Input>> {
        let mut inputs = Vec::new();
        for (i, con_stat) in connect_status.iter().enumerate() {
            if con_stat.disconnected && con_stat.last_frame < frame {
                inputs.push(PlayerInput::blank_input(NULL_FRAME));
            } else {
                inputs.push(self.input_queues[i].confirmed_input(frame));
            }
        }
        inputs
    }

    /// Sets the last confirmed frame to a given frame. By raising the last confirmed frame, we can discard all previous frames, as they are no longer necessary.
    pub(crate) fn set_last_confirmed_frame(&mut self, mut frame: Frame, sparse_saving: bool) {
        // don't set the last confirmed frame after the first incorrect frame before a rollback has happened
        let mut first_incorrect: Frame = NULL_FRAME;
        for handle in 0..self.num_players {
            first_incorrect = std::cmp::max(
                first_incorrect,
                self.input_queues[handle].first_incorrect_frame(),
            );
        }

        // if sparse saving option is turned on, don't set the last confirmed frame after the last saved frame
        if sparse_saving {
            frame = std::cmp::min(frame, self.last_saved_frame);
        }

        // never delete stuff ahead of the current frame
        frame = std::cmp::min(frame, self.current_frame());

        // if we set the last confirmed frame beyond the first incorrect frame, we discard inputs that we need later for adjusting the gamestate.
        assert!(first_incorrect == NULL_FRAME || first_incorrect >= frame);

        self.last_confirmed_frame = frame;
        if self.last_confirmed_frame > 0 {
            for i in 0..self.num_players {
                self.input_queues[i].discard_confirmed_frames(frame - 1);
            }
        }
    }

    /// Finds the earliest incorrect frame detected by the individual input queues
    pub(crate) fn check_simulation_consistency(&self, mut first_incorrect: Frame) -> Frame {
        for handle in 0..self.num_players {
            let incorrect = self.input_queues[handle].first_incorrect_frame();
            if incorrect != NULL_FRAME
                && (first_incorrect == NULL_FRAME || incorrect < first_incorrect)
            {
                first_incorrect = incorrect;
            }
        }
        first_incorrect
    }

    /// Returns a gamestate through given frame
    pub(crate) fn saved_state_by_frame(&self, frame: Frame) -> Option<GameStateCell<T::State>> {
        let cell = self.saved_states.get_cell(frame);

        if cell.0.lock().frame == frame {
            Some(cell)
        } else {
            None
        }
    }

    /// Returns the latest saved frame
    pub(crate) fn last_saved_frame(&self) -> Frame {
        self.last_saved_frame
    }

    /// Returns the latest confirmed frame
    pub(crate) fn last_confirmed_frame(&self) -> Frame {
        self.last_confirmed_frame
    }
}

// #########
// # TESTS #
// #########

#[cfg(test)]
mod sync_layer_tests {

    use super::*;
    use bytemuck::{Pod, Zeroable};
    use std::net::SocketAddr;

    #[repr(C)]
    #[derive(Copy, Clone, PartialEq, Pod, Zeroable)]
    struct TestInput {
        inp: u8,
    }

    struct TestConfig;

    impl Config for TestConfig {
        type Input = TestInput;
        type State = u8;
        type Address = SocketAddr;
    }

    #[test]
    fn test_different_delays() {
        let mut sync_layer = SyncLayer::<TestConfig>::new(2, 8);
        let p1_delay = 2;
        let p2_delay = 0;
        sync_layer.set_frame_delay(0, p1_delay);
        sync_layer.set_frame_delay(1, p2_delay);

        let mut dummy_connect_status = Vec::new();
        dummy_connect_status.push(ConnectionStatus::default());
        dummy_connect_status.push(ConnectionStatus::default());

        for i in 0..20 {
            let game_input = PlayerInput::new(i, TestInput { inp: i as u8 });
            // adding input as remote to avoid prediction threshold detection
            sync_layer.add_remote_input(0, game_input);
            sync_layer.add_remote_input(1, game_input);
            // update the dummy connect status
            dummy_connect_status[0].last_frame = i;
            dummy_connect_status[1].last_frame = i;

            if i >= 3 {
                let sync_inputs = sync_layer.synchronized_inputs(&dummy_connect_status);
                let player0_inputs = sync_inputs[0].0.inp;
                let player1_inputs = sync_inputs[1].0.inp;
                assert_eq!(player0_inputs, i as u8 - p1_delay as u8);
                assert_eq!(player1_inputs, i as u8 - p2_delay as u8);
            }

            sync_layer.advance_frame();
        }
    }
}
