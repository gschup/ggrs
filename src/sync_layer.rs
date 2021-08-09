use parking_lot::Mutex;
use std::sync::Arc;

use crate::error::GGRSError;
use crate::frame_info::{GameInput, GameState, BLANK_INPUT};
use crate::input_queue::InputQueue;
use crate::network::udp_msg::ConnectionStatus;
use crate::{Frame, GGRSRequest, PlayerHandle, MAX_PREDICTION_FRAMES, NULL_FRAME};

/// An `Arc<Mutex<GameState>>` that you can `save()`/`load()` a `GameState` to/from. These will be handed to the user as part of a `GGRSRequest`.
#[derive(Debug)]
pub struct GameStateCell(Arc<Mutex<GameState>>);

impl GameStateCell {
    pub(crate) fn reset(&self) {
        *self.0.lock() = GameState {
            frame: NULL_FRAME,
            buffer: None,
            checksum: 0,
        }
    }

    /// Saves a `GameState` the user creates into the cell.
    pub fn save(&self, new_state: GameState) {
        let mut state = self.0.lock();
        assert!(new_state.frame != NULL_FRAME);
        state.frame = new_state.frame;
        state.checksum = new_state.checksum;
        state.buffer = new_state.buffer;
    }

    /// Loads a `GameState` that the user previously saved into it.
    ///
    /// # Panics
    /// Will panic if the data has previously not been saved to.
    pub fn load(&self) -> GameState {
        let state = self.0.lock();
        if state.frame != NULL_FRAME {
            state.clone()
        } else {
            panic!("Trying to load data that wasn't saved to.")
        }
    }
}

impl Default for GameStateCell {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(GameState::default())))
    }
}

impl Clone for GameStateCell {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SavedStates {
    // the states array is two bigger than the max prediction frames in order to account for
    // the next frame needing a space and still being able to rollback the max distance
    pub states: [GameStateCell; MAX_PREDICTION_FRAMES as usize + 2],
}

impl Default for SavedStates {
    fn default() -> Self {
        Self {
            states: Default::default(),
        }
    }
}

impl SavedStates {
    fn push(&mut self, frame: Frame) -> GameStateCell {
        assert!(frame >= 0);
        let pos = frame as usize % self.states.len();
        let cell = self.states[pos].clone();
        cell.reset();
        cell
    }

    fn peek(&mut self, frame: Frame) -> GameStateCell {
        assert!(frame >= 0);
        let pos = frame as usize % self.states.len();
        let saved_cell = self.states[pos].clone();
        saved_cell
    }

    fn by_frame(&self, frame: Frame) -> Option<GameStateCell> {
        self.states
            .iter()
            .find(|saved| saved.0.lock().frame == frame)
            .cloned()
    }
}

#[derive(Debug)]
pub(crate) struct SyncLayer {
    num_players: u32,
    input_size: usize,
    saved_states: SavedStates,
    rolling_back: bool,
    last_confirmed_frame: Frame,
    last_saved_frame: Frame,
    current_frame: Frame,
    input_queues: Vec<InputQueue>,
}

impl SyncLayer {
    /// Creates a new `SyncLayer` instance with given values.
    pub(crate) fn new(num_players: u32, input_size: usize) -> Self {
        // initialize input_queues
        let mut input_queues = Vec::new();
        for i in 0..num_players {
            input_queues.push(InputQueue::new(i as PlayerHandle, input_size));
        }
        Self {
            num_players,
            input_size,
            rolling_back: false,
            last_confirmed_frame: NULL_FRAME,
            last_saved_frame: NULL_FRAME,
            current_frame: 0,
            saved_states: SavedStates {
                states: Default::default(),
            },
            input_queues,
        }
    }

    pub(crate) const fn current_frame(&self) -> Frame {
        self.current_frame
    }

    pub(crate) fn advance_frame(&mut self) {
        self.current_frame += 1;
    }

    pub(crate) fn save_current_state(&mut self) -> GGRSRequest {
        self.last_saved_frame = self.current_frame;
        let cell = self.saved_states.push(self.current_frame);
        GGRSRequest::SaveGameState {
            cell,
            frame: self.current_frame,
        }
    }

    pub(crate) fn set_frame_delay(&mut self, player_handle: PlayerHandle, delay: u32) {
        assert!(player_handle < self.num_players as PlayerHandle);
        self.input_queues[player_handle as usize].set_frame_delay(delay);
    }

    pub(crate) fn reset_prediction(&mut self) {
        for i in 0..self.num_players {
            self.input_queues[i as usize].reset_prediction();
        }
    }

    /// Loads the gamestate indicated by `frame_to_load`.
    pub(crate) fn load_frame(&mut self, frame_to_load: Frame) -> GGRSRequest {
        // The state should not be the current state or the state should not be in the future or too far away in the past
        assert!(
            frame_to_load != NULL_FRAME
                && frame_to_load < self.current_frame
                && frame_to_load >= self.current_frame - MAX_PREDICTION_FRAMES as i32
        );

        // Reset the head of the state ring-buffer to point in advance of the current frame (as if we had just finished executing it).
        let cell = self.saved_states.peek(frame_to_load);
        let loaded_frame = cell.0.lock().frame;
        assert_eq!(loaded_frame, frame_to_load);

        self.current_frame = loaded_frame;

        GGRSRequest::LoadGameState { cell }
    }

    /// Adds local input to the corresponding input queue. Checks if the prediction threshold has been reached. Returns the frame number where the input is actually added to.
    /// This number will only be different if the input delay was set to a number higher than 0.
    pub(crate) fn add_local_input(
        &mut self,
        player_handle: PlayerHandle,
        input: GameInput,
    ) -> Result<Frame, GGRSError> {
        let frames_ahead = self.current_frame - self.last_confirmed_frame;
        if frames_ahead >= MAX_PREDICTION_FRAMES as i32 {
            return Err(GGRSError::PredictionThreshold);
        }

        // The input provided should match the current frame, we account for input delay later
        assert_eq!(input.frame, self.current_frame);
        Ok(self.input_queues[player_handle].add_input(input))
    }

    /// Adds remote input to the correspoinding input queue.
    /// Unlike `add_local_input`, this will not check for correct conditions, as remote inputs have already been checked on another device.
    pub(crate) fn add_remote_input(&mut self, player_handle: PlayerHandle, input: GameInput) {
        self.input_queues[player_handle].add_input(input);
    }

    /// Returns inputs for all players for the current frame of the sync layer. If there are none for a specific player, return predictions.
    pub(crate) fn synchronized_inputs(
        &mut self,
        connect_status: &[ConnectionStatus],
    ) -> Vec<GameInput> {
        let mut inputs = Vec::new();
        for (i, con_stat) in connect_status.iter().enumerate() {
            if con_stat.disconnected && con_stat.last_frame < self.current_frame {
                inputs.push(BLANK_INPUT);
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
    ) -> Vec<GameInput> {
        let mut inputs = Vec::new();
        for (i, con_stat) in connect_status.iter().enumerate() {
            if con_stat.disconnected && con_stat.last_frame < frame {
                inputs.push(BLANK_INPUT);
            } else {
                inputs.push(self.input_queues[i].confirmed_input(frame));
            }
        }
        inputs
    }

    /// Sets the last confirmed frame to a given frame. By raising the last confirmed frame, we can discard all previous frames, as they are no longer necessary.
    pub(crate) fn set_last_confirmed_frame(&mut self, mut frame: Frame, sparse_saving: bool) {
        // dont set the last confirmed frame after the first incorrect frame before a rollback has happened
        let mut first_incorrect: Frame = NULL_FRAME;
        for handle in 0..self.num_players as usize {
            first_incorrect = std::cmp::max(
                first_incorrect,
                self.input_queues[handle].first_incorrect_frame(),
            );
        }

        // if sparse saving option is turned on, don't set the last confirmed frame after the last saved frame
        if sparse_saving {
            frame = std::cmp::min(frame, self.last_saved_frame);
        }

        // if we set the last confirmed frame beyond the first incorrect frame, we discard inputs that we need later for ajusting the gamestate.
        assert!(first_incorrect == NULL_FRAME || first_incorrect >= frame);

        self.last_confirmed_frame = frame;
        if self.last_confirmed_frame > 0 {
            for i in 0..self.num_players {
                self.input_queues[i as usize].discard_confirmed_frames(frame - 1);
            }
        }
    }

    /// Finds the earliest incorrect frame detected by the individual input queues
    pub(crate) fn check_simulation_consistency(&self, mut first_incorrect: Frame) -> Frame {
        for handle in 0..self.num_players as usize {
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
    pub(crate) fn saved_state_by_frame(&self, frame: Frame) -> Option<GameStateCell> {
        self.saved_states.by_frame(frame)
    }

    /// Returns the latest saved frame
    pub(crate) const fn last_saved_frame(&self) -> Frame {
        self.last_saved_frame
    }
}

// #########
// # TESTS #
// #########

#[cfg(test)]
mod sync_layer_tests {

    use super::*;

    #[test]
    #[should_panic]
    fn test_reach_prediction_threshold() {
        let mut sync_layer = SyncLayer::new(2, std::mem::size_of::<u32>());
        for i in 0..20 {
            let serialized_input = bincode::serialize(&i).unwrap();
            let mut game_input = GameInput::new(i, std::mem::size_of::<u32>());
            game_input.copy_input(&serialized_input);
            sync_layer.add_local_input(0, game_input).unwrap(); // should crash at frame 7
        }
    }

    #[test]
    fn test_different_delays() {
        let mut sync_layer = SyncLayer::new(2, std::mem::size_of::<u32>());
        let p1_delay = 2;
        let p2_delay = 0;
        sync_layer.set_frame_delay(0, p1_delay);
        sync_layer.set_frame_delay(1, p2_delay);

        let mut dummy_connect_status = Vec::new();
        dummy_connect_status.push(ConnectionStatus::default());
        dummy_connect_status.push(ConnectionStatus::default());

        for i in 0..20 {
            let serialized_input = bincode::serialize(&i).unwrap();
            let mut game_input = GameInput::new(i, std::mem::size_of::<u32>());
            game_input.copy_input(&serialized_input);
            // adding input as remote to avoid prediction threshold detection
            sync_layer.add_remote_input(0, game_input);
            sync_layer.add_remote_input(1, game_input);
            // update the dummy connect status
            dummy_connect_status[0].last_frame = i;
            dummy_connect_status[1].last_frame = i;

            if i >= 3 {
                let sync_inputs = sync_layer.synchronized_inputs(&dummy_connect_status);
                let player0_inputs: u32 = bincode::deserialize(&sync_inputs[0].buffer).unwrap();
                let player1_inputs: u32 = bincode::deserialize(&sync_inputs[1].buffer).unwrap();
                assert_eq!(player0_inputs, i as u32 - p1_delay);
                assert_eq!(player1_inputs, i as u32 - p2_delay);
            }

            sync_layer.advance_frame();
        }
    }
}
