use adler::Adler32;
use bincode;
use serde::{Deserialize, Serialize};
use std::hash::Hash;

use ggrs::{GGRSInterface, GameInput, GameState};

pub const INPUT_SIZE: usize = std::mem::size_of::<u32>();

pub struct GameStub {
    pub gs: GameStateStub,
}

impl GameStub {
    #[allow(dead_code)]
    pub fn new() -> GameStub {
        GameStub {
            gs: GameStateStub { frame: 0, state: 0 },
        }
    }
}

#[derive(Hash, Default, Serialize, Deserialize)]
pub struct GameStateStub {
    pub frame: i32,
    pub state: i32,
}

impl GameStateStub {
    fn advance_frame(&mut self, inputs: Vec<GameInput>) {
        let p0_inputs: u32 = bincode::deserialize(inputs[0].input()).unwrap();
        let p1_inputs: u32 = bincode::deserialize(inputs[1].input()).unwrap();

        if (p0_inputs + p1_inputs) % 2 == 0 {
            self.state += 2;
        } else {
            self.state -= 1;
        }
        self.frame += 1;
    }
}

impl GGRSInterface for GameStub {
    fn save_game_state(&self) -> GameState {
        let buffer = bincode::serialize(&self.gs).unwrap();
        let mut adler = Adler32::new();
        self.gs.hash(&mut adler);
        let checksum = adler.checksum();
        GameState {
            frame: self.gs.frame,
            buffer,
            checksum: Some(checksum),
        }
    }

    fn load_game_state(&mut self, state: &GameState) {
        self.gs = bincode::deserialize(&state.buffer).unwrap();
    }

    fn advance_frame(&mut self, inputs: Vec<GameInput>) {
        self.gs.advance_frame(inputs);
    }
}
