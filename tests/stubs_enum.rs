use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::net::SocketAddr;

use ggrs::{Config, Frame, GGRSRequest, GameStateCell, InputStatus, TransparentPad};

fn calculate_hash<T: Hash>(t: &T) -> u64 {
    let mut s = DefaultHasher::new();
    t.hash(&mut s);
    s.finish()
}

pub struct GameStubEnum {
    pub gs: StateStubEnum,
}
use bytemuck::{CheckedBitPattern, NoUninit, Zeroable};

#[repr(u16)]
#[derive(Copy, Clone, PartialEq)]
pub enum EnumInput {
    Val1(u16),
    Val2(TransparentPad<u8, 8>),
}

unsafe impl NoUninit for EnumInput {}

unsafe impl Zeroable for EnumInput {
    fn zeroed() -> Self {
        unsafe { core::mem::zeroed() }
    }
}

unsafe impl CheckedBitPattern for EnumInput {
    type Bits = u32;

    fn is_valid_bit_pattern(bits: &u32) -> bool {
        match *bits {
            0b0 => {
                let alignment = std::mem::align_of::<EnumInput>();
                let view = &bits as *const _ as *const u8;
                let inner = &unsafe { view.offset(alignment as isize) as u16 };
                u16::is_valid_bit_pattern(inner)
            }
            0b1 => {
                let alignment = std::mem::align_of::<EnumInput>();
                let view = &bits as *const _ as *const u8;
                let inner = &unsafe { view.offset(alignment as isize) as u32 };
                let res: Result<&TransparentPad<u8, 8>, _> = bytemuck::checked::try_cast_ref(inner);
                res.is_ok()
            }
            _ => false,
        }
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
