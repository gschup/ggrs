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
    bytes: [0; crate::MAX_INPUT_BYTES],
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

/// This struct holds a byte buffer where the first `size` bytes represent the encoded input of a single player.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
pub struct GameInput {
    /// The frame to which this info belongs to. -1 represents an invalid frame
    pub frame: FrameNumber,
    // The input size per player
    pub size: usize,
    /// The game input for a player in a single frame
    pub bytes: InputBuffer,
}

impl GameInput {
    pub(crate) fn new(frame: FrameNumber, bytes: Option<&InputBuffer>, size: usize) -> Self {
        assert!(size <= MAX_INPUT_BYTES);
        match bytes {
            Some(i_bytes) => Self {
                frame,
                size,
                bytes: *i_bytes,
            },
            None => Self {
                frame,
                size,
                bytes: [0; crate::MAX_INPUT_BYTES],
            },
        }
    }

    pub(crate) fn copy_input(&mut self, bytes: &[u8]) {
        assert!(bytes.len() == self.size);
        self.bytes[0..self.size].copy_from_slice(bytes);
    }

    pub(crate) fn erase_bits(&mut self) {
        self.bytes.iter_mut().for_each(|m| *m = 0)
    }

    pub(crate) fn equal(&self, other: &Self, bitsonly: bool) -> bool {
        (bitsonly || self.frame == other.frame)
            && self.size == other.size
            && self.bytes == other.bytes
    }

    pub fn input(&self) -> &[u8] {
        &self.bytes[0..self.size]
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
