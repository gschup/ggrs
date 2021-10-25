use bincode;
use rand::{prelude::ThreadRng, thread_rng, Rng};
use serde::{Deserialize, Serialize};

use ggrs::{Frame, GGRSRequest, GameInput, GameState, GameStateCell};

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

    #[allow(dead_code)]
    pub fn handle_requests(&mut self, requests: Vec<GGRSRequest>) {
        for request in requests {
            match request {
                GGRSRequest::LoadGameState { cell } => self.load_game_state(cell),
                GGRSRequest::SaveGameState { cell, frame } => self.save_game_state(cell, frame),
                GGRSRequest::AdvanceFrame { inputs } => self.advance_frame(inputs),
            }
        }
    }

    fn save_game_state(&mut self, cell: GameStateCell, frame: Frame) {
        assert_eq!(self.gs.frame, frame);
        let buffer = bincode::serialize(&self.gs).unwrap();

        cell.save(GameState::new(frame, Some(buffer), None));
    }

    fn load_game_state(&mut self, cell: GameStateCell) {
        let game_state = cell.load();
        self.gs = bincode::deserialize(&game_state.buffer.unwrap()).unwrap();
    }

    fn advance_frame(&mut self, inputs: Vec<GameInput>) {
        self.gs.advance_frame(inputs);
    }
}

pub struct RandomChecksumGameStub {
    pub gs: GameStateStub,
    rng: ThreadRng,
}

impl RandomChecksumGameStub {
    #[allow(dead_code)]
    pub fn new() -> RandomChecksumGameStub {
        RandomChecksumGameStub {
            gs: GameStateStub { frame: 0, state: 0 },
            rng: thread_rng(),
        }
    }

    #[allow(dead_code)]
    pub fn handle_requests(&mut self, requests: Vec<GGRSRequest>) {
        for request in requests {
            match request {
                GGRSRequest::LoadGameState { cell } => self.load_game_state(cell),
                GGRSRequest::SaveGameState { cell, frame } => self.save_game_state(cell, frame),
                GGRSRequest::AdvanceFrame { inputs } => self.advance_frame(inputs),
            }
        }
    }

    fn save_game_state(&mut self, cell: GameStateCell, frame: Frame) {
        assert_eq!(self.gs.frame, frame);
        let buffer = bincode::serialize(&self.gs).unwrap();

        let random_checksum: u64 = self.rng.gen();
        cell.save(GameState::new(frame, Some(buffer), Some(random_checksum)));
    }

    fn load_game_state(&mut self, cell: GameStateCell) {
        let game_state = cell.load();
        self.gs = bincode::deserialize(&game_state.buffer.unwrap()).unwrap();
    }

    fn advance_frame(&mut self, inputs: Vec<GameInput>) {
        self.gs.advance_frame(inputs);
    }
}

#[derive(Default, Serialize, Deserialize)]
pub struct GameStateStub {
    pub frame: i32,
    pub state: i32,
}

impl GameStateStub {
    fn advance_frame(&mut self, inputs: Vec<GameInput>) {
        let p0_inputs: u32 = bincode::deserialize(&inputs[0].buffer).unwrap();
        let p1_inputs: u32 = bincode::deserialize(&inputs[0].buffer).unwrap();

        if (p0_inputs + p1_inputs) % 2 == 0 {
            self.state += 2;
        } else {
            self.state -= 1;
        }
        self.frame += 1;
    }
}
