use crate::{GGEZError, circular_buffer::CircularBuffer};
use crate::frame_info::GameState;
use crate::GGEZInterface;

#[derive(Debug, Default)]
pub struct SyncLayer {
    num_players: u32,
    input_size: usize,
    saved_frames: CircularBuffer<GameState>,
    rolling_back: bool,
    last_confirmed_frame: i32,
    frame: u32,
}

impl SyncLayer {
    /// Creates a new [SyncLayer] instance with given values.
    pub fn new(num_players: u32, input_size: usize) -> Self {
        SyncLayer {
            num_players,
            input_size,
            rolling_back: false,
            last_confirmed_frame: -1,
            frame: 0,
            saved_frames: CircularBuffer::new(crate::MAX_PREDICTION_FRAMES as usize),
        }
    }

    pub fn get_frame_count(&self) -> u32 {
        self.frame
    }

    pub fn save_current_frame(&mut self, interface: &mut impl GGEZInterface) {
        self.saved_frames.push_back(interface.save_game_state());
    }

    pub fn get_last_saved_frame(&self) -> Option<&GameState> {
        self.saved_frames.front()
    }

    pub fn advance_frame(&mut self, interface: &mut impl GGEZInterface) {
        self.frame += 1;
        self.save_current_frame(interface);
    }

    pub fn load_frame(&mut self, interface: &mut impl GGEZInterface, frame_to_load: u32) -> Result<(), GGEZError>{
        // The state is already loaded
        if self.frame == frame_to_load {
            return Ok(());
        }
        // go backwards through the queue
        while !self.saved_frames.is_empty() {  
            match self.saved_frames.front() {
                Some(state) => {
                    if state.frame == frame_to_load {
                        interface.load_game_state(state)
                    } else {
                        self.saved_frames.pop_front();
                    }
                },
                None => continue
            }
        }

        Ok(())
    }
}
