use crate::game_info::GameInput;
use crate::game_info::GameState;
use crate::input_queue::InputQueue;
use crate::{
    FrameNumber, GGEZError, GGEZInterface, PlayerHandle, MAX_INPUT_DELAY, MAX_PREDICTION_FRAMES,
    NULL_FRAME,
};
#[derive(Debug, Clone)]
struct SavedStates {
    states: [GameState; MAX_PREDICTION_FRAMES as usize + 2],
    head: usize,
}

const BLANK_STATE: GameState = GameState {
    frame: NULL_FRAME,
    buffer: Vec::new(),
    checksum: None,
};

#[derive(Debug)]
pub struct SyncLayer {
    num_players: u32,
    input_size: usize,
    saved_states: SavedStates,
    rolling_back: bool,
    last_confirmed_frame: FrameNumber,
    current_frame: FrameNumber,
    input_queues: Vec<InputQueue>,
}

impl SyncLayer {
    /// Creates a new [SyncLayer] instance with given values.
    pub fn new(num_players: u32, input_size: usize) -> SyncLayer {
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
                states: [BLANK_STATE; MAX_PREDICTION_FRAMES + 2],
            },
            input_queues: input_queues,
        }
    }

    pub fn get_current_frame(&self) -> FrameNumber {
        self.current_frame
    }

    pub fn advance_frame(&mut self) {
        self.current_frame += 1;
    }

    pub fn save_current_state(&mut self, interface: &impl GGEZInterface) {
        let state_to_save = interface.save_game_state();
        self.saved_states.head = (self.saved_states.head + 1) % self.saved_states.states.len();
        assert!(state_to_save.frame != NULL_FRAME);
        self.saved_states.states[self.saved_states.head] = state_to_save;
    }

    pub fn get_last_saved_state(&self) -> Option<&GameState> {
        match self.saved_states.states[self.saved_states.head].frame {
            NULL_FRAME => None,
            _ => Some(&self.saved_states.states[self.saved_states.head]),
        }
    }

    pub fn set_frame_delay(
        &mut self,
        player_handle: PlayerHandle,
        delay: u32,
    ) -> Result<(), GGEZError> {
        if player_handle >= self.num_players as PlayerHandle {
            return Err(GGEZError::InvalidPlayerHandle);
        }
        if delay > MAX_INPUT_DELAY {
            return Err(GGEZError::InvalidRequest);
        }

        self.input_queues[player_handle as usize].set_frame_delay(delay);
        Ok(())
    }

    /// Searches the saved states and returns the index of the state that matches the given frame number. If not found, returns an [GGEZError].
    fn find_saved_frame_index(&self, frame: FrameNumber) -> Result<usize, GGEZError> {
        let count = self.saved_states.states.len();

        for i in 0..count {
            if self.saved_states.states[i].frame == frame {
                return Ok(i);
            }
        }
        Err(GGEZError::GeneralFailure)
    }

    /// Loads the gamestate indicated by the frame_to_load. After execution, `self.saved_states.head` is set to the loaded state.
    pub fn load_frame(
        &mut self,
        interface: &mut impl GGEZInterface,
        frame_to_load: FrameNumber,
    ) -> Result<(), GGEZError> {
        // The state is the current state (not yet saved) or the state cannot possibly be inside our queue since it is too far away in the past
        if self.current_frame == frame_to_load
            || frame_to_load == NULL_FRAME
            || frame_to_load > self.current_frame
            || frame_to_load < self.current_frame - MAX_PREDICTION_FRAMES as i32
        {
            return Err(GGEZError::InvalidRequest);
        }

        self.saved_states.head = self.find_saved_frame_index(frame_to_load)?;
        let state_to_load = &self.saved_states.states[self.saved_states.head];

        assert_eq!(state_to_load.frame, frame_to_load);
        interface.load_game_state(state_to_load);

        Ok(())
    }

    pub fn add_local_input(
        &mut self,
        player_handle: PlayerHandle,
        input: &GameInput,
    ) -> Result<(), GGEZError> {
        let frames_behind = self.current_frame - self.last_confirmed_frame;
        if frames_behind >= MAX_PREDICTION_FRAMES as i32 {
            return Err(GGEZError::PredictionThreshold);
        }

        if input.frame != self.current_frame {
            return Err(GGEZError::GeneralFailure);
        }

        self.input_queues[player_handle as PlayerHandle].add_input(input);
        Ok(())
    }

    pub fn add_remote_input(
        &mut self,
        player_handle: PlayerHandle,
        input: &GameInput,
    ) -> Result<(), GGEZError> {
        self.input_queues[player_handle as PlayerHandle].add_input(input);
        Ok(())
    }
}
