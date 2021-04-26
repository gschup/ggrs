use crate::{FrameNumber, InputBuffer, NULL_FRAME};

#[derive(Debug, Clone)]
pub struct FrameInfo {
    pub frame: FrameNumber,
    pub state: GameState,
    pub input: GameInput,
}

#[derive(Debug, Default, Clone)]
pub struct GameState {
    /// The frame to which this info belongs to.
    pub frame: FrameNumber,
    /// The serialized gamestate in bytes.
    pub buffer: Vec<u8>,
    /// The checksum of the gamestate.
    pub checksum: Option<u32>,
}

impl GameState {
    pub fn new() -> GameState {
        GameState {
            frame: NULL_FRAME,
            buffer: Vec::new(),
            checksum: None,
        }
    }
}

/// All input data for all players for a single frame is saved in this struct.
#[derive(Debug, Default, Copy, Clone)]
pub struct GameInput {
    /// The frame to which this info belongs to. -1 represents an invalid frame
    pub frame: FrameNumber,
    // The input size per player
    pub size: usize,
    /// The game input for a player in a single frame
    pub bits: InputBuffer,
}

impl GameInput {
    pub fn new(frame: FrameNumber, bits: Option<&InputBuffer>, size: usize) -> GameInput {
        assert!(size <= crate::MAX_INPUT_BYTES);
        match bits {
            Some(i_bits) => GameInput {
                frame,
                size,
                bits: i_bits.clone(),
            },
            None => GameInput {
                frame,
                size,
                bits: [0; crate::MAX_INPUT_BYTES],
            },
        }
    }

    pub fn add_input(&mut self, bits: &[u8]) {
        assert!(bits.len() <= crate::MAX_INPUT_BYTES);
        self.bits[0..self.size].copy_from_slice(bits);
    }

    pub fn erase_bits(&mut self) {
        self.bits.iter_mut().for_each(|m| *m = 0)
    }

    pub fn equal(&self, other: &GameInput, bitsonly: bool) -> bool {
        (bitsonly || self.frame == other.frame)
            && self.size == other.size
            && self.bits == other.bits
    }
}
