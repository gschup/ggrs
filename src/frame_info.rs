use crate::{Frame, NULL_FRAME};

/// Represents the game state of your game for a single frame. The `data` holds your state, `frame` indicates the associated frame number
/// and `checksum` can additionally be provided for use during a `SyncTestSession`.
/// You are expected to return this during `save_game_state()` and use them during `load_game_state()`.
#[derive(Debug, Clone)]
pub struct GameState<S: Clone> {
    /// The frame to which this info belongs to.
    pub frame: Frame,
    /// The game state
    pub data: Option<S>,
    /// The checksum of the gamestate.
    pub checksum: u64,
}

impl<S: Clone> Default for GameState<S> {
    fn default() -> Self {
        Self {
            frame: NULL_FRAME,
            data: None,
            checksum: 0,
        }
    }
}

impl<S: Clone> GameState<S> {
    pub fn new(frame: Frame, data: Option<S>) -> Self {
        Self {
            frame,
            data,
            checksum: 0,
        }
    }
}

impl<S: Clone> GameState<S> {
    pub fn new_with_checksum(frame: Frame, data: Option<S>, checksum: u64) -> Self {
        Self {
            frame,
            data,
            checksum,
        }
    }
}

/// Represents an input for a single player in a single frame. The associated frame is denoted with `frame`.
/// You do not need to create this struct, but the sessions will provide a `Vec<GameInput>` for you during `advance_frame()`.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PlayerInput<I>
where
    I: Copy + Clone + PartialEq + bytemuck::Pod + bytemuck::Zeroable,
{
    /// The frame to which this info belongs to. -1/`NULL_FRAME` represents an invalid frame
    pub frame: Frame,
    /// The input struct given by the user
    pub input: I,
}

impl<I: Copy + Clone + PartialEq + bytemuck::Pod + bytemuck::Zeroable> PlayerInput<I> {
    /// Returns true, if the player of that input has disconnected
    pub fn is_disconnected(&self) -> bool {
        self.frame == NULL_FRAME
    }

    pub(crate) fn new(frame: Frame, input: I) -> Self {
        Self { frame, input }
    }

    pub(crate) fn blank_input(frame: Frame) -> Self {
        Self {
            frame,
            input: I::zeroed(),
        }
    }

    pub(crate) fn equal(&self, other: &Self, input_only: bool) -> bool {
        (input_only || self.frame == other.frame) && self.input == other.input
    }
}

// #########
// # TESTS #
// #########

#[cfg(test)]
mod game_input_tests {
    use super::*;
    use bytemuck::{Pod, Zeroable};

    #[repr(C)]
    #[derive(Copy, Clone, PartialEq, Pod, Zeroable)]
    struct TestInput {
        inp: u8,
    }

    #[test]
    fn test_input_equality() {
        let input1 = PlayerInput::new(0, TestInput { inp: 5 });
        let input2 = PlayerInput::new(0, TestInput { inp: 5 });
        assert!(input1.equal(&input2, false));
    }

    #[test]
    fn test_input_equality_input_only() {
        let input1 = PlayerInput::new(0, TestInput { inp: 5 });
        let input2 = PlayerInput::new(5, TestInput { inp: 5 });
        assert!(input1.equal(&input2, true)); // different frames, but does not matter
    }

    #[test]
    fn test_input_equality_fail() {
        let input1 = PlayerInput::new(0, TestInput { inp: 5 });
        let input2 = PlayerInput::new(0, TestInput { inp: 7 });
        assert!(!input1.equal(&input2, false)); // different bits
    }
}
