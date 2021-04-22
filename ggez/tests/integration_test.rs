use adler::Adler32;
use bincode;
use std::hash::Hash;
use serde::{Serialize, Deserialize};

use ggez::{GGEZEvent, GGEZInterface, GGEZSession};
use ggez::sessions::sync_test::SyncTestSession;
use ggez::frame_info::{GameState, GameInput};
use ggez::player::{Player, PlayerType};

struct GameStub {
    gs: GameStateStub,
    sess: SyncTestSession,
}

impl GameStub {
    fn new() -> GameStub {
        GameStub {
            gs: GameStateStub {
                frame: 0,
                state: 0,
            },
            sess: ggez::start_synctest_session(1, 2, std::mem::size_of::<u32>()),
        }
    }
}

#[derive(Hash, Default, Serialize, Deserialize)]
struct GameStateStub {
    pub frame: u32,
    pub state: u32,
}

impl GameStateStub {
    fn advance_frame(&mut self, _inputs: &GameInput) {
        // we ignore the inputs for now
        self.frame += 1;
        self.state += 2;
    }
}

impl GGEZInterface for GameStub {
    fn save_game_state(&self) -> GameState {
        let buffer = bincode::serialize(&self.gs).unwrap();
        let mut adler = Adler32::new();
        self.gs.hash(&mut adler);
        let checksum = Some(adler.checksum());
        GameState {
            frame: self.gs.frame,
            buffer,
            checksum,
        }
    }

    fn load_game_state(&mut self, state: &GameState) {
        self.gs = bincode::deserialize(&state.buffer).unwrap();
    }

    fn advance_frame(&mut self, inputs: &GameInput, _disconnect_flags: u32) {
        self.gs.advance_frame(inputs);
    }

    fn on_event(&mut self, info: GGEZEvent) {
        println!("{:?}", info);
    }
}

#[test]
fn test_start_synctest_session() {
    let mut stub = GameStub::new();
    let player = Player::new(PlayerType::Local, 1);
    let _handle = stub.sess.add_player(&player).unwrap();
}
