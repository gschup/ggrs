use crate::{FrameNumber, MAX_INPUT_BYTES, NULL_FRAME};

pub type InputBuffer = [u8; MAX_INPUT_BYTES];

pub const BLANK_FRAME: FrameInfo = FrameInfo {
    frame: NULL_FRAME,
    state: BLANK_STATE,
    input: BLANK_INPUT,
};

pub const BLANK_STATE: GameState = GameState {
    frame: NULL_FRAME,
    buffer: Vec::new(),
    checksum: None,
};

pub const BLANK_INPUT: GameInput = GameInput {
    frame: NULL_FRAME,
    bits: [0; crate::MAX_INPUT_BYTES],
    size: 0,
};

/// This struct holds a state and an input. It is intended that both the state and the input correspond to the same frame.
#[derive(Debug, Clone, Default)]
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
    pub const fn new() -> Self {
        Self {
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
    pub fn new(frame: FrameNumber, bits: Option<&InputBuffer>, size: usize) -> Self {
        assert!(size <= crate::MAX_INPUT_BYTES);
        match bits {
            Some(i_bits) => Self {
                frame,
                size,
                bits: *i_bits,
            },
            None => Self {
                frame,
                size,
                bits: [0; crate::MAX_INPUT_BYTES],
            },
        }
    }

    pub fn copy_input(&mut self, bits: &[u8]) {
        assert!(bits.len() <= crate::MAX_INPUT_BYTES);
        self.bits[0..self.size].copy_from_slice(bits);
    }

    pub fn erase_bits(&mut self) {
        self.bits.iter_mut().for_each(|m| *m = 0)
    }

    pub fn equal(&self, other: &Self, bitsonly: bool) -> bool {
        (bitsonly || self.frame == other.frame)
            && self.size == other.size
            && self.bits == other.bits
    }
}

// #########
// # TESTS #
// #########

#[cfg(test)]
mod game_input_tests {
    use super::*;
    use bincode;

    #[test]
    fn test_input_equality_bits_only() {
        let fake_inputs: u32 = 5;
        let input_size = std::mem::size_of::<u32>();
        let serialized_inputs = bincode::serialize(&fake_inputs).unwrap();
        let mut input1 = GameInput::new(0, None, input_size);
        input1.copy_input(&serialized_inputs);
        let mut input2 = GameInput::new(5, None, input_size);
        input2.copy_input(&serialized_inputs);
        assert!(input1.equal(&input2, true)); // different frames, but does not matter
    }

    #[test]
    fn test_input_equality_fail() {
        let input_size = std::mem::size_of::<u32>();

        let fake_inputs: u32 = 5;
        let serialized_inputs = bincode::serialize(&fake_inputs).unwrap();
        let mut input1 = GameInput::new(0, None, input_size);
        input1.copy_input(&serialized_inputs);

        let fake_inputs: u32 = 7;
        let serialized_inputs = bincode::serialize(&fake_inputs).unwrap();
        let mut input2 = GameInput::new(0, None, input_size);
        input2.copy_input(&serialized_inputs);

        assert!(!input1.equal(&input2, false)); // different bits
    }
}
