mod stubs;

use ggrs::{P2PSession, PlayerType, SessionState, UdpNonBlockingSocket};
use serial_test::serial;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use stubs::{StubConfig, StubInput};

#[test]
#[serial]
fn test_create_session() {
    let socket = UdpNonBlockingSocket::bind_to_port(7777).unwrap();
    let _sess = P2PSession::<StubConfig>::new(2, stubs::MAX_PRED_FRAMES, socket);
}

#[test]
#[serial]
fn test_add_player() {
    let socket = UdpNonBlockingSocket::bind_to_port(7777).unwrap();
    let mut sess = P2PSession::<StubConfig>::new(2, stubs::MAX_PRED_FRAMES, socket);
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
    assert!(sess.add_player(PlayerType::Local, 0).is_ok());
    assert!(sess.add_player(PlayerType::Remote(addr), 1).is_ok());
    assert!(sess.add_player(PlayerType::Remote(addr), 1).is_err()); // handle already registered
    assert!(sess.add_player(PlayerType::Remote(addr), 2).is_err()); // invalid handle
    assert!(sess.add_player(PlayerType::Spectator(addr), 2).is_ok());
    assert!(sess.add_player(PlayerType::Spectator(addr), 2).is_err()); // specatator handle already registered
    assert!(sess.start_session().is_ok());
    assert!(sess.add_player(ggrs::PlayerType::Remote(addr), 1).is_err()); // cannot add player after starting
}

#[test]
#[serial]
fn test_start_session() {
    let socket = UdpNonBlockingSocket::bind_to_port(7777).unwrap();
    let mut sess = P2PSession::<StubConfig>::new(2, stubs::MAX_PRED_FRAMES, socket);
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
    let socket = UdpNonBlockingSocket::bind_to_port(7777).unwrap();
    let mut sess = P2PSession::<StubConfig>::new(2, stubs::MAX_PRED_FRAMES, socket);
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
    let socket1 = UdpNonBlockingSocket::bind_to_port(7777).unwrap();
    let mut sess1 = P2PSession::<StubConfig>::new(2, stubs::MAX_PRED_FRAMES, socket1);
    let socket2 = UdpNonBlockingSocket::bind_to_port(8888).unwrap();
    let mut sess2 = P2PSession::<StubConfig>::new(2, stubs::MAX_PRED_FRAMES, socket2);
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
        sess1.poll_remote_clients();
        sess2.poll_remote_clients();
    }

    assert!(sess1.current_state() == SessionState::Running);
    assert!(sess2.current_state() == SessionState::Running);
}

#[test]
#[serial]
fn test_advance_frame_p2p_sessions() {
    let mut stub1 = stubs::GameStub::new();
    let mut stub2 = stubs::GameStub::new();
    let socket1 = UdpNonBlockingSocket::bind_to_port(7777).unwrap();
    let mut sess1 = P2PSession::<StubConfig>::new(2, stubs::MAX_PRED_FRAMES, socket1);
    let socket2 = UdpNonBlockingSocket::bind_to_port(8888).unwrap();
    let mut sess2 = P2PSession::<StubConfig>::new(2, stubs::MAX_PRED_FRAMES, socket2);
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
        sess1.poll_remote_clients();
        sess2.poll_remote_clients();
    }

    assert!(sess1.current_state() == SessionState::Running);
    assert!(sess2.current_state() == SessionState::Running);

    let reps = 10;
    for i in 0..reps {
        sess1.poll_remote_clients();
        sess2.poll_remote_clients();

        let requests1 = sess1.advance_frame(0, StubInput { inp: i }).unwrap();
        stub1.handle_requests(requests1);
        let requests2 = sess2.advance_frame(1, StubInput { inp: i }).unwrap();
        stub2.handle_requests(requests2);

        // gamestate evolves
        assert_eq!(stub1.gs.frame, i as i32 + 1);
        assert_eq!(stub2.gs.frame, i as i32 + 1);
    }
}
