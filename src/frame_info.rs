use crate::{FrameNumber, MAX_INPUT_BYTES, NULL_FRAME};

pub type InputBuffer = [u8; MAX_INPUT_BYTES];

pub(crate) const BLANK_FRAME: FrameInfo = FrameInfo {
    frame: NULL_FRAME,
    state: BLANK_STATE,
    input: BLANK_INPUT,
};

pub(crate) const BLANK_STATE: GameState = GameState {
    frame: NULL_FRAME,
    buffer: Vec::new(),
    checksum: None,
};

pub const BLANK_INPUT: GameInput = GameInput {
    frame: NULL_FRAME,
    buffer: [0; MAX_INPUT_BYTES],
    size: 0,
};

/// This struct holds a state and an input. It is intended that both the state and the input correspond to the same frame.
#[derive(Debug, Clone, Default)]
pub(crate) struct FrameInfo {
    pub frame: FrameNumber,
    pub state: GameState,
    pub input: GameInput,
}

/// Represents a serialized game state of your game for a single frame. The buffer `buffer` holds your state, `frame` indicates the associated frame number
/// and `checksum` can additionally be provided for use during a `SyncTestSession`. You are expected to return this during `save_game_state()` and use them during `load_game_state()`.
#[derive(Debug, Clone)]
pub struct GameState {
    /// The frame to which this info belongs to.
    pub frame: FrameNumber,
    /// The serialized gamestate in bytes.
    pub buffer: Vec<u8>,
    /// The checksum of the gamestate.
    pub checksum: Option<u32>,
}

impl Default for GameState {
    fn default() -> Self {
        Self {
            frame: NULL_FRAME,
            buffer: Vec::new(),
            checksum: None,
        }
    }
}

impl GameState {
    pub fn new() -> Self {
        GameState::default()
    }
}

/// Represents a serialized input for a single player in a single frame. This struct holds a `buffer` where the first `size` bytes represent the encoded input of a single player.
/// The associated frame is denoted with `frame`. You do not need to create this struct, but the sessions will provide a `Vec<GameInput>` for you during `advance_frame()`.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
pub struct GameInput {
    /// The frame to which this info belongs to. -1/`NULL_FRAME` represents an invalid frame
    pub frame: FrameNumber,
    // The input size per player
    pub size: usize,
    /// The game input for a player in a single frame
    pub buffer: InputBuffer,
}

impl GameInput {
    pub(crate) fn new(frame: FrameNumber, bytes: Option<&InputBuffer>, size: usize) -> Self {
        assert!(size <= MAX_INPUT_BYTES);
        match bytes {
            Some(i_bytes) => Self {
                frame,
                size,
                buffer: *i_bytes,
            },
            None => Self {
                frame,
                size,
                buffer: [0; crate::MAX_INPUT_BYTES],
            },
        }
    }

    pub(crate) fn copy_input(&mut self, bytes: &[u8]) {
        assert!(bytes.len() == self.size);
        self.buffer[0..self.size].copy_from_slice(bytes);
    }

    pub(crate) fn erase_bits(&mut self) {
        self.buffer.iter_mut().for_each(|m| *m = 0)
    }

    pub(crate) fn equal(&self, other: &Self, bitsonly: bool) -> bool {
        (bitsonly || self.frame == other.frame)
            && self.size == other.size
            && self.buffer == other.buffer
    }

    /// Retrieve your serialized input with this method. Returns a slice which you can use to deserialize.
    pub fn input(&self) -> &[u8] {
        &self.buffer[0..self.size]
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
