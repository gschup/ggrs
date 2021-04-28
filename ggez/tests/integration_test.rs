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
    pub frame: i32,
    pub state: u32,
}

impl GameStateStub {
    fn advance_frame(&mut self, inputs: Vec<GameInput>) {
        let player0_inputs: u32 = bincode::deserialize(&inputs[1].bits).unwrap();
        if player0_inputs % 2 == 0 {
            self.state += 2;
        } else {
            self.state += 1;
        }
        self.frame += 1;
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

    fn advance_frame(&mut self, inputs: Vec<GameInput>, _disconnect_flags: u8) {
        self.gs.advance_frame(inputs);
    }

    fn on_event(&mut self, info: GGEZEvent) {
        println!("{:?}", info);
    }
}

#[test]
fn test_add_player() {
    let mut sess = ggez::start_synctest_session(1, 2, std::mem::size_of::<u32>());

    // add players correctly
    let dummy_player_0 = Player::new(PlayerType::Local, 0);
    let dummy_player_1 = Player::new(PlayerType::Local, 1);

    match sess.add_player(&dummy_player_0) {
        Ok(handle) => assert_eq!(handle, 0),
        Err(_) => assert!(false),
    }

    match sess.add_player(&dummy_player_1) {
        Ok(handle) => assert_eq!(handle, 1),
        Err(_) => assert!(false),
    }
}

#[test]
fn test_add_player_invalid_handle() {
    let mut sess = ggez::start_synctest_session(1, 2, std::mem::size_of::<u32>());

    // add a player incorrectly
    let incorrect_player = Player::new(PlayerType::Local, 3);

    assert!(sess.add_player(&incorrect_player).is_err());
}

#[test]
fn test_add_local_input_not_running() {
    let mut sess = ggez::start_synctest_session(1, 2, std::mem::size_of::<u32>());

    // add 0 input for player 0
    let fake_inputs: u32 = 0;
    let serialized_inputs = bincode::serialize(&fake_inputs).unwrap();

    assert!(sess.add_local_input(0, &serialized_inputs).is_err());
}

#[test]
fn test_add_local_input_invalid_handle() {
    let mut sess = ggez::start_synctest_session(1, 2, std::mem::size_of::<u32>());
    sess.start_session().unwrap();

    // add 0 input for player 3
    let fake_inputs: u32 = 0;
    let serialized_inputs = bincode::serialize(&fake_inputs).unwrap();

    assert!(sess.add_local_input(3, &serialized_inputs).is_err());
}

#[test]
fn test_start_synctest_session() {
    let mut sess = ggez::start_synctest_session(1, 2, std::mem::size_of::<u32>());
    let player = Player::new(PlayerType::Local, 1);
    let handle = sess.add_player(&player).unwrap();
    assert_eq!(handle, 1);
    sess.start_session().unwrap();
}

#[test]
fn test_advance_frame() {
    let mut stub = GameStub::new();
    let mut sess = ggez::start_synctest_session(7, 2, std::mem::size_of::<u32>());
    let player = Player::new(PlayerType::Local, 1);
    let handle = sess.add_player(&player).unwrap();
    assert_eq!(handle, 1);
    sess.start_session().unwrap();

    for i in 0..100 {
        let input: u32 = i;
        let serialized_input = bincode::serialize(&input).unwrap();
        sess.add_local_input(handle, &serialized_input).unwrap();
        sess.advance_frame(&mut stub).unwrap();
        assert_eq!(stub.gs.frame, i as i32 + 1); // frame should have advanced
    }
}

#[test]
fn test_advance_frames_with_delayed_input() {
    let mut stub = GameStub::new();
    let mut sess = ggez::start_synctest_session(7, 2, std::mem::size_of::<u32>());
    let player = Player::new(PlayerType::Local, 1);
    let handle = sess.add_player(&player).unwrap();
    assert_eq!(handle, 1);
    sess.set_frame_delay(2, handle).unwrap();
    sess.start_session().unwrap();

    for i in 0..100 {
        let input: u32 = i;
        let serialized_input = bincode::serialize(&input).unwrap();
        sess.add_local_input(handle, &serialized_input).unwrap();
        sess.advance_frame(&mut stub).unwrap();
        assert_eq!(stub.gs.frame, i as i32 + 1); // frame should have advanced
    }
}
