use crate::{Frame, NULL_FRAME};

/// Computes the fletcher16 checksum, copied from wikipedia: <https://en.wikipedia.org/wiki/Fletcher%27s_checksum>
fn fletcher16(data: &[u8]) -> u16 {
    let mut sum1: u16 = 0;
    let mut sum2: u16 = 0;

    for byte in data {
        sum1 = (sum1 + *byte as u16) % 255;
        sum2 = (sum2 + sum1) % 255;
    }

    (sum2 << 8) | sum1
}

/// Represents a serialized game state of your game for a single frame. The `data` holds your state, `frame` indicates the associated frame number
/// and `checksum` can additionally be provided for use during a `SyncTestSession` (requires feature `sync_test`).
/// You are expected to return this during `save_game_state()` and use them during `load_game_state()`.
#[derive(Debug, Clone)]
pub struct GameState<T: Clone> {
    /// The frame to which this info belongs to.
    pub frame: Frame,
    /// The serialized gamestate in bytes.
    pub data: Option<T>,
    /// The checksum of the gamestate.
    #[cfg(feature = "sync_test")]
    pub checksum: u64,
}

impl<T: Clone> Default for GameState<T> {
    fn default() -> Self {
        Self {
            frame: NULL_FRAME,
            data: None,
            #[cfg(feature = "sync_test")]
            checksum: 0,
        }
    }
}

impl<T: Clone> GameState<T> {
    pub fn new(frame: Frame, data: Option<T>) -> Self {
        Self {
            frame,
            data,
            #[cfg(feature = "sync_test")]
            checksum: 0,
        }
    }
}

#[cfg(feature = "sync_test")]
impl<T: Clone + AsRef<[u8]>> GameState<T> {
    pub fn new_with_checksum(frame: Frame, data: Option<T>, check: Option<u64>) -> Self {
        let checksum = match check {
            Some(cs) => cs,
            None => match &data {
                Some(data) => fletcher16(data.as_ref()) as u64,
                None => 0,
            },
        };

        Self {
            frame,
            data,
            checksum,
        }
    }
}

/// Represents a serialized input for a single player in a single frame. This struct holds a `data` which represents the encoded input of a single player.
/// The associated frame is denoted with `frame`. You do not need to create this struct, but the sessions will provide a `Vec<GameInput>` for you during `advance_frame()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameInput {
    /// The frame to which this info belongs to. -1/`NULL_FRAME` represents an invalid frame
    pub frame: Frame,
    // The input size
    pub size: usize,
    /// An input buffer that will hold input data
    pub buffer: Vec<u8>,
}

impl GameInput {
    pub(crate) fn new(frame: Frame, size: usize, buffer: Vec<u8>) -> Self {
        assert!(size == buffer.len());

        Self {
            frame,
            size,
            buffer,
        }
    }

    pub(crate) fn blank_input(size: usize) -> Self {
        Self {
            frame: NULL_FRAME,
            size,
            buffer: vec![0; size],
        }
    }
}

impl GameInput {
    pub(crate) fn erase_bits(&mut self) {
        self.buffer = vec![0; self.size];
    }

    pub(crate) fn equal(&self, other: &Self, bitsonly: bool) -> bool {
        (bitsonly || self.frame == other.frame)
            && self.size == other.size
            && self.buffer == other.buffer
    }
}

// #########
// # TESTS #
// #########

#[cfg(test)]
mod game_input_tests {
    use super::*;

    #[test]
    fn test_input_equality_bits_only() {
        let fake_inputs: u32 = 5;
        let input_size = std::mem::size_of::<u32>();
        let serialized_inputs = bincode::serialize(&fake_inputs).unwrap();
        let input1 = GameInput::new(0, input_size, serialized_inputs);
        let serialized_inputs = bincode::serialize(&fake_inputs).unwrap();
        let input2 = GameInput::new(5, input_size, serialized_inputs);
        assert!(input1.equal(&input2, true)); // different frames, but does not matter
    }

    #[test]
    fn test_input_equality_fail() {
        let input_size = std::mem::size_of::<u32>();

        let fake_inputs: u32 = 5;
        let serialized_inputs = bincode::serialize(&fake_inputs).unwrap();
        let input1 = GameInput::new(0, input_size, serialized_inputs);

        let fake_inputs: u32 = 7;
        let serialized_inputs = bincode::serialize(&fake_inputs).unwrap();
        let input2 = GameInput::new(0, input_size, serialized_inputs);

        assert!(!input1.equal(&input2, false)); // different bits
    }
}
