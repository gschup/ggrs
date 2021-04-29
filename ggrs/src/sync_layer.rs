use crate::error::GGRSError;
use crate::frame_info::GameInput;
use crate::frame_info::{GameState, BLANK_STATE};
use crate::input_queue::InputQueue;
use crate::{FrameNumber, PlayerHandle, MAX_INPUT_DELAY, MAX_PREDICTION_FRAMES, NULL_FRAME};
#[derive(Debug, Clone)]
pub(crate) struct SavedStates<T> {
    pub states: [T; MAX_PREDICTION_FRAMES],
    pub head: usize,
}

impl<T> SavedStates<T> {
    pub(crate) fn save_state(&mut self, state_to_save: T) {
        self.head = (self.head + 1) % self.states.len();
        self.states[self.head] = state_to_save;
    }

    pub(crate) fn state_at_head(&self) -> &T {
        &self.states[self.head]
    }

    pub(crate) fn state_in_past(&self, frames_in_past: usize) -> &T {
        let pos =
            (self.head as i64 - frames_in_past as i64).rem_euclid(MAX_PREDICTION_FRAMES as i64);
        assert!(pos >= 0);
        &self.states[pos as usize]
    }
}

#[derive(Debug)]
pub(crate) struct SyncLayer {
    num_players: u32,
    input_size: usize,
    saved_states: SavedStates<GameState>,
    rolling_back: bool,
    last_confirmed_frame: FrameNumber,
    current_frame: FrameNumber,
    input_queues: Vec<InputQueue>,
}

impl SyncLayer {
    /// Creates a new [SyncLayer] instance with given values.
    pub(crate) fn new(num_players: u32, input_size: usize) -> SyncLayer {
        // initialize input_queues
        let mut input_queues = Vec::new();
        for i in 0..num_players {
            input_queues.push(InputQueue::new(i as PlayerHandle, input_size));
        }
        SyncLayer {
            num_players,
            input_size,
            rolling_back: false,
            last_confirmed_frame: -1,
            current_frame: 0,
            saved_states: SavedStates {
                head: 0,
                states: [BLANK_STATE; MAX_PREDICTION_FRAMES],
            },
            input_queues: input_queues,
        }
    }

    pub(crate) fn current_frame(&self) -> FrameNumber {
        self.current_frame
    }

    pub(crate) fn advance_frame(&mut self) {
        self.current_frame += 1;
    }

    pub(crate) fn save_current_state(&mut self, state_to_save: GameState) {
        assert!(state_to_save.frame != NULL_FRAME);
        self.saved_states.save_state(state_to_save)
    }

    pub(crate) fn last_saved_state(&self) -> Option<&GameState> {
        match self.saved_states.state_at_head().frame {
            NULL_FRAME => None,
            _ => Some(self.saved_states.state_at_head()),
        }
    }

    pub(crate) fn set_frame_delay(&mut self, player_handle: PlayerHandle, delay: u32) {
        assert!(player_handle < self.num_players as PlayerHandle);
        assert!(delay <= MAX_INPUT_DELAY);

        self.input_queues[player_handle as usize].set_frame_delay(delay);
    }

    pub(crate) fn reset_prediction(&mut self, frame: FrameNumber) {
        for i in 0..self.num_players {
            self.input_queues[i as usize].reset_prediction(frame);
        }
    }

    /// Loads the gamestate indicated by the frame_to_load. After execution, `self.saved_states.head` is set one position after the loaded state.
    pub(crate) fn load_frame(&mut self, frame_to_load: FrameNumber) -> &GameState {
        // The state should not be the current state or the state should not be in the future or too far away in the past
        assert!(
            frame_to_load != NULL_FRAME
                && frame_to_load < self.current_frame
                && frame_to_load >= self.current_frame - MAX_PREDICTION_FRAMES as i32
        );

        self.saved_states.head = self.find_saved_frame_index(frame_to_load);
        let state_to_load = &self.saved_states.states[self.saved_states.head];
        assert_eq!(state_to_load.frame, frame_to_load);

        // Reset framecount and the head of the state ring-buffer to point in
        // advance of the current frame (as if we had just finished executing it).
        self.saved_states.head = (self.saved_states.head + 1) % MAX_PREDICTION_FRAMES;
        self.current_frame = frame_to_load;

        state_to_load
    }

    /// Adds local input to the corresponding input queue. Checks if the prediction threshold has been reached.
    pub(crate) fn add_local_input(
        &mut self,
        player_handle: PlayerHandle,
        input: GameInput,
    ) -> Result<(), GGRSError> {
        let frames_behind = self.current_frame - self.last_confirmed_frame;
        if frames_behind > MAX_PREDICTION_FRAMES as i32 {
            return Err(GGRSError::PredictionThresholdError);
        }

        // The input provided should match the current frame
        assert_eq!(input.frame, self.current_frame);
        self.input_queues[player_handle].add_input(input);
        Ok(())
    }

    /// Adds remote input to the correspoinding input queue.
    /// Unlike `add_local_input`, this will not check for correct conditions, as remote inputs have already been checked on another device.
    pub(crate) fn add_remote_input(&mut self, player_handle: PlayerHandle, input: GameInput) {
        self.input_queues[player_handle].add_input(input);
    }

    /// Returns inputs for all players for the current frame of the sync layer. If there are none for a specific player, return predictions.
    pub(crate) fn synchronized_inputs(&mut self) -> Vec<GameInput> {
        let mut inputs = Vec::new();
        for i in 0..self.num_players {
            inputs.push(self.input_queues[i as usize].input(self.current_frame));
        }
        inputs
    }

    /// Returns confirmed inputs for all players for the current frame of the sync layer.
    pub(crate) fn confirmed_inputs(&mut self) -> Vec<GameInput> {
        let mut inputs = Vec::new();
        for i in 0..self.num_players {
            inputs.push(self.input_queues[i as usize].confirmed_input(self.current_frame));
        }
        inputs
    }

    /// Sets the last confirmed frame to a given frame. By raising the last confirmed frame, we can discard all previous frames, as they are no longer necessary.
    pub(crate) fn set_last_confirmed_frame(&mut self, frame: FrameNumber) {
        self.last_confirmed_frame = frame;
        if self.last_confirmed_frame > 0 {
            for i in 0..self.num_players {
                self.input_queues[i as usize].discard_confirmed_frames(frame - 1);
            }
        }
    }

    /// Searches the saved states and returns the index of the state that matches the given frame number.
    fn find_saved_frame_index(&self, frame: FrameNumber) -> usize {
        for i in 0..MAX_PREDICTION_FRAMES {
            if self.saved_states.states[i].frame == frame {
                return i;
            }
        }
        panic!("SyncLayer::find_saved_frame_index(): requested state could not be found");
    }
}
