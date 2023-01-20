use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::net::SocketAddr;

use ggrs::{Config, Frame, GGRSRequest, GameStateCell, InputStatus};

fn calculate_hash<T: Hash>(t: &T) -> u64 {
    let mut s = DefaultHasher::new();
    t.hash(&mut s);
    s.finish()
}

pub struct GameStubEnum {
    pub gs: StateStubEnum,
}
use bytemuck::{CheckedBitPattern, NoUninit, Zeroable};

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, CheckedBitPattern, NoUninit)]
pub enum EnumInput {
    Val1,
    Val2,
}

unsafe impl Zeroable for EnumInput {
    fn zeroed() -> Self {
        unsafe { core::mem::zeroed() }
    }
}

pub struct StubEnumConfig;

impl Config for StubEnumConfig {
    type Input = EnumInput;
    type State = StateStubEnum;
    type Address = SocketAddr;
}

impl GameStubEnum {
    #[allow(dead_code)]
    pub fn new() -> GameStubEnum {
        GameStubEnum {
            gs: StateStubEnum { frame: 0, state: 0 },
        }
    }

    #[allow(dead_code)]
    pub fn handle_requests(&mut self, requests: Vec<GGRSRequest<StubEnumConfig>>) {
        for request in requests {
            match request {
                GGRSRequest::LoadGameState { cell, .. } => self.load_game_state(cell),
                GGRSRequest::SaveGameState { cell, frame } => self.save_game_state(cell, frame),
                GGRSRequest::AdvanceFrame { inputs } => self.advance_frame(inputs),
            }
        }
    }

    fn save_game_state(&mut self, cell: GameStateCell<StateStubEnum>, frame: Frame) {
        assert_eq!(self.gs.frame, frame);
        let checksum = calculate_hash(&self.gs);
        cell.save(frame, Some(self.gs), Some(checksum as u128));
    }

    fn load_game_state(&mut self, cell: GameStateCell<StateStubEnum>) {
        self.gs = cell.load().unwrap();
    }

    fn advance_frame(&mut self, inputs: Vec<(EnumInput, InputStatus)>) {
        self.gs.advance_frame(inputs);
    }
}

#[derive(Default, Copy, Clone, Hash)]
pub struct StateStubEnum {
    pub frame: i32,
    pub state: i32,
}

impl StateStubEnum {
    fn advance_frame(&mut self, inputs: Vec<(EnumInput, InputStatus)>) {
        let p0_inputs = inputs[0];
        let p1_inputs = inputs[1];

        if p0_inputs == p1_inputs {
            self.state += 2;
        } else {
            self.state -= 1;
        }
        self.frame += 1;
    }
}
