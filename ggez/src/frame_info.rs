/// All information for a single frame of the gamestate
#[derive(Debug, Default, Clone)]
pub struct FrameInfo {
    /// The frame to which this info belongs to.
    pub frame: u32,
    /// The saved game state
    pub state: GameState,
    /// The input data of all players for this frame.
    pub input: GameInput,
}

#[derive(Debug, Default, Clone)]
pub struct GameState {
    /// The serialized gamestate in bytes.
    pub buffer: Vec<u8>,
    /// The checksum of the gamestate.
    pub checksum: Option<u32>,
}

/// All input data for all players for a single frame is saved in this struct.
#[derive(Debug, Default, Clone)]
pub struct GameInput {
    /// The game input for all players
    pub input_bits: Vec<u8>,
}

impl GameInput {
    pub fn new(input_size: usize, bits: Option<&[u8]>) -> GameInput {
        match bits {
            Some(i_bits) => GameInput {
                input_bits: i_bits.to_vec(),
            },
            None => GameInput {
                input_bits: vec![0; input_size],
            },
        }
    }

    pub fn erase_bits(&mut self) {
        for i in 0..self.input_bits.len() {
            self.input_bits[i] = 0;
        }
    }
}
