use crate::{FrameNumber, MAX_INPUT_BYTES, MAX_PLAYERS, NULL_FRAME};

/// The input buffer used to save the bytes from a player input. It is bigger than necessary by a factor `MAX_PLAYERS` to allow the same type of buffers to be used to transmit
/// player inputs for all players to the spectators. This definitely isn't optimal and might be changed later.
pub type InputBuffer = [u8; MAX_INPUT_BYTES * MAX_PLAYERS as usize];

pub(crate) const BLANK_STATE: GameState = GameState {
    frame: NULL_FRAME,
    buffer: Vec::new(),
    checksum: None,
};

pub const BLANK_INPUT: GameInput = GameInput {
    frame: NULL_FRAME,
    buffer: [0; MAX_INPUT_BYTES * MAX_PLAYERS as usize],
    size: 0,
};

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
    // The input size
    pub size: usize,
    /// An input buffer that will hold input data
    pub buffer: InputBuffer,
}

impl GameInput {
    pub(crate) fn new(frame: FrameNumber, bytes: Option<&InputBuffer>, size: usize) -> Self {
        assert!(size > 0);
        match bytes {
            Some(i_bytes) => Self {
                frame,
                size,
                buffer: *i_bytes,
            },
            None => Self {
                frame,
                size,
                buffer: [0; MAX_INPUT_BYTES * MAX_PLAYERS as usize],
            },
        }
    }
}

impl GameInput {
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
