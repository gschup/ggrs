use bincode;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use ggrs::player::{Player, PlayerType};
use ggrs::GGRSSession;

mod stubs;

#[test]
fn test_create_session() {
    ggrs::start_synctest_session(2, std::mem::size_of::<u32>(), 1).unwrap();
}

#[test]
fn test_add_player() {
    let mut sess = ggrs::start_synctest_session(2, std::mem::size_of::<u32>(), 1).unwrap();

    // add players correctly
    let dummy_player_0 = Player::new(PlayerType::Local, 0);
    let dummy_player_1 = Player::new(PlayerType::Local, 1);

    assert!(sess.add_player(&dummy_player_0).is_ok());
    assert!(sess.add_player(&dummy_player_1).is_ok());
}

#[test]
fn test_add_player_invalid_handle() {
    let mut sess = ggrs::start_synctest_session(2, std::mem::size_of::<u32>(), 1).unwrap();

    // add a player incorrectly
    let incorrect_player = Player::new(PlayerType::Local, 3);

    assert!(sess.add_player(&incorrect_player).is_err());
}

#[test]
fn test_add_player_invalid_player_type_for_synctest() {
    let mut sess = ggrs::start_synctest_session(2, std::mem::size_of::<u32>(), 1).unwrap();

    // add a player incorrectly
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
    let incorrect_player = Player::new(PlayerType::Remote(addr), 0);

    assert!(sess.add_player(&incorrect_player).is_err());
}

#[test]
fn test_add_local_input_not_running() {
    let mut sess = ggrs::start_synctest_session(2, std::mem::size_of::<u32>(), 1).unwrap();

    // add 0 input for player 0
    let fake_inputs: u32 = 0;
    let serialized_inputs = bincode::serialize(&fake_inputs).unwrap();

    assert!(sess.add_local_input(0, &serialized_inputs).is_err());
}

#[test]
fn test_add_local_input_invalid_handle() {
    let mut sess = ggrs::start_synctest_session(2, std::mem::size_of::<u32>(), 1).unwrap();
    sess.start_session().unwrap();

    // add 0 input for player 3
    let fake_inputs: u32 = 0;
    let serialized_inputs = bincode::serialize(&fake_inputs).unwrap();

    assert!(sess.add_local_input(3, &serialized_inputs).is_err());
}

#[test]
fn test_start_synctest_session() {
    let mut sess = ggrs::start_synctest_session(2, std::mem::size_of::<u32>(), 1).unwrap();
    let player = Player::new(PlayerType::Local, 1);
    sess.add_player(&player).unwrap();
    sess.start_session().unwrap();
}

#[test]
fn test_advance_frame() {
    let handle = 1;
    let check_distance = 7;
    let mut stub = stubs::GameStub::new();
    let mut sess =
        ggrs::start_synctest_session(2, std::mem::size_of::<u32>(), check_distance).unwrap();
    let player = Player::new(PlayerType::Local, handle);
    sess.add_player(&player).unwrap();
    sess.start_session().unwrap();

    for i in 0..200 {
        let input: u32 = i;
        let serialized_input = bincode::serialize(&input).unwrap();
        sess.add_local_input(handle, &serialized_input).unwrap();
        sess.advance_frame(&mut stub).unwrap();
        assert_eq!(stub.gs.frame, i as i32 + 1); // frame should have advanced
    }
}

#[test]
fn test_advance_frames_with_delayed_input() {
    let handle = 1;
    let check_distance = 7;
    let mut stub = stubs::GameStub::new();
    let mut sess =
        ggrs::start_synctest_session(2, std::mem::size_of::<u32>(), check_distance).unwrap();
    let player = Player::new(PlayerType::Local, 1);
    sess.add_player(&player).unwrap();
    sess.set_frame_delay(2, handle).unwrap();
    sess.start_session().unwrap();

    for i in 0..200 {
        let input: u32 = i;
        let serialized_input = bincode::serialize(&input).unwrap();
        sess.add_local_input(handle, &serialized_input).unwrap();
        sess.advance_frame(&mut stub).unwrap();
        assert_eq!(stub.gs.frame, i as i32 + 1); // frame should have advanced
    }
}
