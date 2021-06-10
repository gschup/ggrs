use ggrs::{GGRSSession, SessionState};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use serial_test::serial;

mod stubs;

#[test]
#[serial]
fn test_create_session() {
    assert!(ggrs::start_p2p_session(2, stubs::INPUT_SIZE, 7777).is_ok());
}

#[test]
#[serial]
fn test_add_player() {
    let mut sess = ggrs::start_p2p_session(2, stubs::INPUT_SIZE, 7777).unwrap();
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
    assert!(sess.add_player(ggrs::PlayerType::Local, 0).is_ok());
    assert!(sess.add_player(ggrs::PlayerType::Remote(addr), 1).is_ok());
    assert!(sess.add_player(ggrs::PlayerType::Remote(addr), 1).is_err()); // handle already registered
    assert!(sess.add_player(ggrs::PlayerType::Remote(addr), 2).is_err()); // invalid handle
    assert!(sess.start_session().is_ok());
    assert!(sess.add_player(ggrs::PlayerType::Remote(addr), 1).is_err()); // cannot add player after starting
}

#[test]
#[serial]
fn test_start_session() {
    let mut sess = ggrs::start_p2p_session(2, stubs::INPUT_SIZE, 7777).unwrap();
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
    assert!(sess.add_player(ggrs::PlayerType::Local, 0).is_ok());
    assert!(sess.start_session().is_err()); // not enough players
    assert!(sess.add_player(ggrs::PlayerType::Remote(addr), 1).is_ok());
    assert!(sess.start_session().is_ok()); // works
    assert!(sess.start_session().is_err()); // cannot start twice
}

#[test]
#[serial]
fn test_disconnect_player() {
    let mut sess = ggrs::start_p2p_session(2, stubs::INPUT_SIZE, 7777).unwrap();
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
    assert!(sess.add_player(ggrs::PlayerType::Local, 0).is_ok());
    assert!(sess.add_player(ggrs::PlayerType::Remote(addr), 1).is_ok());
    assert!(sess.start_session().is_ok());
    assert!(sess.disconnect_player(0).is_err()); // for now, local players cannot be disconnected
    assert!(sess.disconnect_player(1).is_ok());
    assert!(sess.disconnect_player(1).is_err()); // already disconnected
}

#[test]
#[serial]
fn test_synchronize_p2p_sessions() {
    let mut sess1 = ggrs::start_p2p_session(2, stubs::INPUT_SIZE, 7777).unwrap();
    let mut sess2 = ggrs::start_p2p_session(2, stubs::INPUT_SIZE, 8888).unwrap();
    let addr1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 7777);
    let addr2 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8888);

    assert!(sess1.current_state() == SessionState::Initializing);
    assert!(sess2.current_state() == SessionState::Initializing);

    assert!(sess1.add_player(ggrs::PlayerType::Local, 0).is_ok());
    assert!(sess1.add_player(ggrs::PlayerType::Remote(addr2), 1).is_ok());
    assert!(sess1.start_session().is_ok());

    assert!(sess2.add_player(ggrs::PlayerType::Local, 1).is_ok());
    assert!(sess2.add_player(ggrs::PlayerType::Remote(addr1), 0).is_ok());
    assert!(sess2.start_session().is_ok());

    assert!(sess1.current_state() == SessionState::Synchronizing);
    assert!(sess2.current_state() == SessionState::Synchronizing);

    for _ in 0..10 {
        sess1.idle();
        sess2.idle();
    }

    assert!(sess1.current_state() == SessionState::Running);
    assert!(sess2.current_state() == SessionState::Running);
}

#[test]
#[serial]
fn test_advance_frame_p2p_sessions() {
    let mut stub1 = stubs::GameStub::new();
    let mut stub2 = stubs::GameStub::new();
    let mut sess1 = ggrs::start_p2p_session(2, stubs::INPUT_SIZE, 7777).unwrap();
    let mut sess2 = ggrs::start_p2p_session(2, stubs::INPUT_SIZE, 8888).unwrap();
    let addr1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 7777);
    let addr2 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8888);

    assert!(sess1.current_state() == SessionState::Initializing);
    assert!(sess2.current_state() == SessionState::Initializing);

    assert!(sess1.add_player(ggrs::PlayerType::Local, 0).is_ok());
    assert!(sess1.add_player(ggrs::PlayerType::Remote(addr2), 1).is_ok());
    assert!(sess1.start_session().is_ok());

    assert!(sess2.add_player(ggrs::PlayerType::Local, 1).is_ok());
    assert!(sess2.add_player(ggrs::PlayerType::Remote(addr1), 0).is_ok());
    assert!(sess2.start_session().is_ok());

    assert!(sess1.current_state() == SessionState::Synchronizing);
    assert!(sess2.current_state() == SessionState::Synchronizing);

    for _ in 0..10 {
        sess1.idle();
        sess2.idle();
    }

    assert!(sess1.current_state() == SessionState::Running);
    assert!(sess2.current_state() == SessionState::Running);

    let reps = 10;
    for i in 0..reps {
        let input: u32 = i;
        let serialized_input = bincode::serialize(&input).unwrap();
        assert!(sess1.add_local_input(0, &serialized_input).is_ok());
        assert!(sess2.add_local_input(1, &serialized_input).is_ok());

        sess1.idle();
        sess2.idle();

        assert!(sess1.advance_frame(&mut stub1).is_ok());
        assert!(sess2.advance_frame(&mut stub2).is_ok());

        // gamestate evolves
        assert_eq!(stub1.gs.frame, i as i32 + 1);
        assert_eq!(stub2.gs.frame, i as i32 + 1);
    }
}
