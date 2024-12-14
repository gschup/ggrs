use rand::{prelude::ThreadRng, thread_rng, Rng};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::net::SocketAddr;

use ggrs::{Config, Frame, GameStateCell, GgrsRequest, InputStatus};

fn calculate_hash<T: Hash>(t: &T) -> u64 {
    let mut s = DefaultHasher::new();
    t.hash(&mut s);
    s.finish()
}

pub struct GameStub {
    pub gs: StateStub,
}

#[repr(C)]
#[derive(Copy, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct StubInput {
    pub inp: u32,
}

pub struct StubConfig;

impl Config for StubConfig {
    type Input = StubInput;
    type State = StateStub;
    type Address = SocketAddr;
}

impl GameStub {
    #[allow(dead_code)]
    pub fn new() -> GameStub {
        GameStub {
            gs: StateStub { frame: 0, state: 0 },
        }
    }

    #[allow(dead_code)]
    pub fn handle_requests(&mut self, requests: Vec<GgrsRequest<StubConfig>>) {
        for request in requests {
            match request {
                GgrsRequest::LoadGameState { cell, .. } => self.load_game_state(cell),
                GgrsRequest::SaveGameState { cell, frame } => self.save_game_state(cell, frame),
                GgrsRequest::AdvanceFrame { inputs } => self.advance_frame(inputs),
            }
        }
    }

    fn save_game_state(&mut self, cell: GameStateCell<StateStub>, frame: Frame) {
        assert_eq!(self.gs.frame, frame);
        let checksum = calculate_hash(&self.gs);
        cell.save(frame, Some(self.gs), Some(checksum as u128));
    }

    fn load_game_state(&mut self, cell: GameStateCell<StateStub>) {
        self.gs = cell.load().unwrap();
    }

    fn advance_frame(&mut self, inputs: Vec<(StubInput, InputStatus)>) {
        self.gs.advance_frame(inputs);
    }
}

pub struct RandomChecksumGameStub {
    pub gs: StateStub,
    rng: ThreadRng,
}

impl RandomChecksumGameStub {
    #[allow(dead_code)]
    pub fn new() -> RandomChecksumGameStub {
        RandomChecksumGameStub {
            gs: StateStub { frame: 0, state: 0 },
            rng: thread_rng(),
        }
    }

    #[allow(dead_code)]
    pub fn handle_requests(&mut self, requests: Vec<GgrsRequest<StubConfig>>) {
        for request in requests {
            match request {
                GgrsRequest::LoadGameState { cell, .. } => self.load_game_state(cell),
                GgrsRequest::SaveGameState { cell, frame } => self.save_game_state(cell, frame),
                GgrsRequest::AdvanceFrame { inputs } => self.advance_frame(inputs),
            }
        }
    }

    fn save_game_state(&mut self, cell: GameStateCell<StateStub>, frame: Frame) {
        assert_eq!(self.gs.frame, frame);

        let random_checksum: u128 = self.rng.gen();
        cell.save(frame, Some(self.gs), Some(random_checksum));
    }

    fn load_game_state(&mut self, cell: GameStateCell<StateStub>) {
        self.gs = cell.load().expect("No data found.");
    }

    fn advance_frame(&mut self, inputs: Vec<(StubInput, InputStatus)>) {
        self.gs.advance_frame(inputs);
    }
}

#[derive(Default, Copy, Clone, Hash)]
pub struct StateStub {
    pub frame: i32,
    pub state: i32,
}

impl StateStub {
    fn advance_frame(&mut self, inputs: Vec<(StubInput, InputStatus)>) {
        let p0_inputs = inputs[0].0.inp;
        let p1_inputs = inputs[1].0.inp;

        if (p0_inputs + p1_inputs) % 2 == 0 {
            self.state += 2;
        } else {
            self.state -= 1;
        }
        self.frame += 1;
    }
}
