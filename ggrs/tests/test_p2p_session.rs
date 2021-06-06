use ggrs::player::{Player, PlayerType};
use ggrs::GGRSSession;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use serial_test::serial;

mod stubs;

#[test]
#[serial]
fn test_create_session() {
    ggrs::start_p2p_session(2, std::mem::size_of::<u32>(), 7777).unwrap();
}

#[test]
#[serial]
fn test_add_player() {
    let mut sess = ggrs::start_p2p_session(2, std::mem::size_of::<u32>(), 7777).unwrap();

    // add players correctly
    let dummy_player_0 = Player::new(PlayerType::Local, 0);
    let dummy_player_1 = Player::new(PlayerType::Local, 1);

    assert!(sess.add_player(&dummy_player_0).is_ok());
    assert!(sess.add_player(&dummy_player_1).is_ok());
}

#[test]
#[serial]
fn test_add_player_local_and_remote() {
    let mut sess = ggrs::start_p2p_session(2, std::mem::size_of::<u32>(), 7777).unwrap();

    let local_dummy_player = Player::new(PlayerType::Local, 0);

    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
    let remote_dummy_player = Player::new(PlayerType::Remote(addr), 1);

    assert!(sess.add_player(&local_dummy_player).is_ok());
    assert!(sess.add_player(&remote_dummy_player).is_ok());
}

#[test]
#[serial]
fn test_add_player_twice() {
    let mut sess = ggrs::start_p2p_session(2, std::mem::size_of::<u32>(), 7777).unwrap();

    let dummy_player_0 = Player::new(PlayerType::Local, 0);
    let dummy_player_1 = Player::new(PlayerType::Local, 1);

    assert!(sess.add_player(&dummy_player_0).is_ok());
    assert!(sess.add_player(&dummy_player_1).is_ok());
    assert!(sess.add_player(&dummy_player_1).is_err());
}

#[test]
#[serial]
fn test_add_player_incorrect_handle() {
    let mut sess = ggrs::start_p2p_session(2, std::mem::size_of::<u32>(), 7777).unwrap();

    let dummy_player_0 = Player::new(PlayerType::Local, 0);
    //incorrect handle
    let dummy_player_1 = Player::new(PlayerType::Local, 2);

    assert!(sess.add_player(&dummy_player_0).is_ok());
    assert!(sess.add_player(&dummy_player_1).is_err());
}

#[test]
#[serial]
fn test_start_session() {
    let mut sess = ggrs::start_p2p_session(2, std::mem::size_of::<u32>(), 7777).unwrap();

    let local_dummy_player = Player::new(PlayerType::Local, 0);

    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
    let remote_dummy_player = Player::new(PlayerType::Remote(addr), 1);

    assert!(sess.add_player(&local_dummy_player).is_ok());
    assert!(sess.add_player(&remote_dummy_player).is_ok());

    assert!(sess.start_session().is_ok());
}

#[test]
#[serial]
fn test_start_session_twice() {
    let mut sess = ggrs::start_p2p_session(2, std::mem::size_of::<u32>(), 7777).unwrap();

    let local_dummy_player = Player::new(PlayerType::Local, 0);

    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
    let remote_dummy_player = Player::new(PlayerType::Remote(addr), 1);

    assert!(sess.add_player(&local_dummy_player).is_ok());
    assert!(sess.add_player(&remote_dummy_player).is_ok());

    assert!(sess.start_session().is_ok());
    assert!(sess.start_session().is_err());
}

#[test]
#[serial]
fn test_start_session_not_enough_players() {
    let mut sess = ggrs::start_p2p_session(2, std::mem::size_of::<u32>(), 7777).unwrap();

    // add players correctly
    let dummy_player_0 = Player::new(PlayerType::Local, 0);

    assert!(sess.add_player(&dummy_player_0).is_ok());
    assert!(sess.start_session().is_err());
}
