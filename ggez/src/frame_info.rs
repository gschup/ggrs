/// All information for a single frame of the gamestate
pub struct FrameInfo {
    /// The frame this information belongs to.
    pub frame: u32,
    /// The checksum of the gamestate.
    pub checksum: u32,
    /// The serialized gamestate in bytes.
    pub buffer: Vec<u8>,
    /// The input data of all players for this frame.
    pub input: GameInput,
}

/// All input data for all players for a single frame is saved in this struct.
pub struct GameInput {
    /// Frame to which this input belongs to.
    pub frame: i32,
    /// The game input for all players
    pub input_bits: Vec<u8>,
}

impl GameInput {
    pub fn new(frame: i32, input_size: usize, bits: Option<&[u8]>) -> GameInput {
        match bits {
            Some(i_bits) => GameInput {
                frame,
                input_bits: i_bits.to_vec(),
            },
            None => GameInput {
                frame,
                input_bits: vec![0; input_size],
            },
        }
    }
}
