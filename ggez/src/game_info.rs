#[derive(Debug, Default, Clone)]
pub struct GameState {
    /// The frame to which this info belongs to.
    pub frame: u32,
    /// The serialized gamestate in bytes.
    pub buffer: Vec<u8>,
    /// The checksum of the gamestate.
    pub checksum: Option<u32>,
}

pub type Input = [u8; crate::MAX_INPUT_BYTES];
pub type InputBuffer = [Input; crate::MAX_PLAYERS];

/// All input data for all players for a single frame is saved in this struct.
#[derive(Debug, Default, Clone)]
pub struct GameInput {
    /// The frame to which this info belongs to.
    pub frame: u32,
    // The input size per player
    pub size_per_player: usize,
    /// The game input for all players, each player gets their own array
    pub bits: InputBuffer,
}

impl GameInput {
    pub fn new(frame: u32, bits: Option<&InputBuffer>, size_per_player: usize) -> GameInput {
        assert!(size_per_player <= crate::MAX_INPUT_BYTES);
        match bits {
            Some(i_bits) => GameInput {
                frame,
                size_per_player,
                bits: i_bits.clone(),
            },
            None => GameInput {
                frame,
                size_per_player,
                bits: [[0; crate::MAX_INPUT_BYTES]; crate::MAX_PLAYERS],
            },
        }
    }

    pub fn add_input_for_player(&mut self, player_handle: usize, bits: &[u8]) {
        assert!(player_handle <= crate::MAX_PLAYERS);
        assert!(bits.len() <= crate::MAX_INPUT_BYTES);
        self.bits[player_handle][0..self.size_per_player].copy_from_slice(bits);
    }

    pub fn erase_bits(&mut self) {
        self.bits = [[0; crate::MAX_INPUT_BYTES]; crate::MAX_PLAYERS];
        for elem in self.bits.iter_mut() {
            elem.iter_mut().for_each(|m| *m = 0)
        }
    }
}
