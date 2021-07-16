use parking_lot::Mutex;
use std::sync::Arc;

use crate::error::GGRSError;
use crate::frame_info::{GameInput, GameState, BLANK_INPUT};
use crate::input_queue::InputQueue;
use crate::network::udp_msg::ConnectionStatus;
use crate::{Frame, GGRSRequest, PlayerHandle, MAX_PREDICTION_FRAMES, NULL_FRAME};

/// An `Rc<RefCell<GameState>>` that you can `save()`/`load()` a `GameState` to/from. These will be handed to the user as part of a `GGRSRequest`.
#[derive(Debug)]
pub struct GameStateCell(Arc<Mutex<GameState>>);

impl GameStateCell {
    pub(crate) fn reset(&self, frame: Frame) {
        *self.0.lock() = GameState {
            frame,
            ..Default::default()
        }
    }

    /// Saves a `GameState` the user creates into the cell.
    pub fn save(&self, new_state: GameState) {
        let mut state = self.0.lock();
        assert!(new_state.buffer.is_some());
        assert_eq!(state.frame, new_state.frame);
        state.checksum = new_state.checksum;
        state.buffer = new_state.buffer;
    }

    /// Loads a `GameState` that the user previously saved into it.
    ///
    /// # Panics
    /// Will panic if the data has previously not been saved to.
    pub fn load(&self) -> GameState {
        let state = self.0.lock();
        if state.buffer.is_some() && state.frame != NULL_FRAME {
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
    pub states: [GameStateCell; MAX_PREDICTION_FRAMES as usize],
    pub head: usize,
}

impl Default for SavedStates {
    fn default() -> Self {
        Self {
            head: 0,
            states: Default::default(),
        }
    }
}

impl SavedStates {
    fn push(&mut self, frame: Frame) -> GameStateCell {
        let saved_state = self.states[self.head].clone();
        saved_state.reset(frame);
        self.head = (self.head + 1) % self.states.len();
        assert!(self.head < self.states.len());
        saved_state
    }

    fn find_index(&self, frame: Frame) -> Option<usize> {
        self.states
            .iter()
            .enumerate()
            .find(|(_, saved)| saved.0.lock().frame == frame)
            .map(|(i, _)| i)
    }

    fn reset_to(&mut self, frame: Frame) -> GameStateCell {
        self.head = self
            .find_index(frame)
            .unwrap_or_else(|| panic!("Could not find saved frame index for frame: {}", frame));
        self.states[self.head].clone()
    }

    #[allow(dead_code)]
    fn latest(&self) -> Option<GameStateCell> {
        self.states
            .iter()
            .filter(|saved| saved.0.lock().frame != NULL_FRAME)
            .max_by_key(|saved| saved.0.lock().frame)
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
            current_frame: 0,
            saved_states: SavedStates {
                head: 0,
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

    pub(crate) fn reset_prediction(&mut self, frame: Frame) {
        for i in 0..self.num_players {
            self.input_queues[i as usize].reset_prediction(frame);
        }
    }

    /// Loads the gamestate indicated by `frame_to_load`. After execution, `self.saved_states.head` is set one position after the loaded state.
    pub(crate) fn load_frame(&mut self, frame_to_load: Frame) -> GGRSRequest {
        // The state should not be the current state or the state should not be in the future or too far away in the past
        assert!(
            frame_to_load != NULL_FRAME
                && frame_to_load < self.current_frame
                && frame_to_load >= self.current_frame - MAX_PREDICTION_FRAMES as i32
        );

        // Reset the head of the state ring-buffer to point in advance of the current frame (as if we had just finished executing it).
        let cell = self.saved_states.reset_to(frame_to_load);
        let loaded_frame = cell.0.lock().frame;
        assert_eq!(loaded_frame, frame_to_load);

        self.saved_states.head = (self.saved_states.head + 1) % MAX_PREDICTION_FRAMES as usize;
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
            if con_stat.disconnected {
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
            if con_stat.disconnected || con_stat.last_frame < frame {
                inputs.push(BLANK_INPUT);
            } else {
                inputs.push(self.input_queues[i].confirmed_input(frame as u32));
            }
        }
        inputs
    }

    /// Sets the last confirmed frame to a given frame. By raising the last confirmed frame, we can discard all previous frames, as they are no longer necessary.
    pub(crate) fn set_last_confirmed_frame(&mut self, frame: Frame) {
        // dont set the last confirmed frame after the first incorrect frame before a rollback has happened
        let mut first_incorrect: Frame = NULL_FRAME;
        for handle in 0..self.num_players as usize {
            first_incorrect = std::cmp::max(
                first_incorrect,
                self.input_queues[handle].first_incorrect_frame(),
            );
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

    pub(crate) fn check_simulation_consistency(&self) -> Option<Frame> {
        let mut first_incorrect: Frame = NULL_FRAME;
        for handle in 0..self.num_players as usize {
            first_incorrect = std::cmp::max(
                first_incorrect,
                self.input_queues[handle].first_incorrect_frame(),
            );
        }
        match first_incorrect {
            NULL_FRAME => None,
            _ => Some(first_incorrect),
        }
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
