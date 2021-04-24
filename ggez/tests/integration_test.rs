use adler::Adler32;
use bincode;
use serde::{Deserialize, Serialize};
use std::hash::Hash;

use ggez::game_info::{GameInput, GameState};
use ggez::player::{Player, PlayerType};
use ggez::{GGEZEvent, GGEZInterface, GGEZSession};

struct GameStub {
    gs: GameStateStub,
}

impl GameStub {
    fn new() -> GameStub {
        GameStub {
            gs: GameStateStub { frame: 0, state: 0 },
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

    fn advance_frame(&mut self, inputs: &GameInput, _disconnect_flags: u32) {
        self.gs.advance_frame(inputs);
    }

    fn on_event(&mut self, info: GGEZEvent) {
        println!("{:?}", info);
    }
}

#[test]
fn test_start_synctest_session() {
    //let mut stub = GameStub::new();
    let mut sess = ggez::start_synctest_session(1, 2, std::mem::size_of::<u32>());
    let player = Player::new(PlayerType::Local, 1);
    let handle = sess.add_player(&player).unwrap();
    assert_eq!(handle, 1);
    sess.start_session().unwrap();
}

#[test]
fn test_advance_frame() {
    let mut stub = GameStub::new();
    let mut sess = ggez::start_synctest_session(1, 2, std::mem::size_of::<u32>());
    let player = Player::new(PlayerType::Local, 1);
    let handle = sess.add_player(&player).unwrap();
    assert_eq!(handle, 1);
    sess.start_session().unwrap();

    for i in 0..10 {
        let input: u32 = i;
        let serialized_input = bincode::serialize(&input).unwrap();
        sess.add_local_input(handle, &serialized_input).unwrap();
        sess.advance_frame(&mut stub).unwrap();
        assert_eq!(stub.gs.frame, i + 1); // frame should have advanced
    }
}
