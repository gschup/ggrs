mod stubs;

use ggrs::{GGRSError, P2PSessionBuilder, PlayerType, SessionState, UdpNonBlockingSocket};
use serial_test::serial;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use stubs::{StubConfig, StubInput};

#[test]
#[serial]
fn test_create_session_builder() {
    let socket = UdpNonBlockingSocket::bind_to_port(7777).unwrap();
    let _sess = P2PSessionBuilder::<StubConfig>::new(2, socket);
}

#[test]
#[serial]
fn test_add_valid_players() -> Result<(), GGRSError> {
    let socket = UdpNonBlockingSocket::bind_to_port(7777).unwrap();
    let remote_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
    let spec_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8090);

    let _sess = P2PSessionBuilder::<StubConfig>::new(2, socket)
        .add_player(PlayerType::Local, 0)?
        .add_player(PlayerType::Remote(remote_addr), 1)?
        .add_player(PlayerType::Spectator(spec_addr), 2);
    Ok(())
}

#[test]
#[serial]
fn test_start_session() -> Result<(), GGRSError> {
    let socket = UdpNonBlockingSocket::bind_to_port(7777).unwrap();
    let remote_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
    let spec_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8090);

    let _sess = P2PSessionBuilder::<StubConfig>::new(2, socket)
        .add_player(PlayerType::Local, 0)?
        .add_player(PlayerType::Remote(remote_addr), 1)?
        .add_player(PlayerType::Spectator(spec_addr), 2)?
        .start_session()?;
    Ok(())
}

#[test]
#[serial]
fn test_disconnect_player() -> Result<(), GGRSError> {
    let socket = UdpNonBlockingSocket::bind_to_port(7777).unwrap();
    let remote_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
    let spec_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8090);

    let mut sess = P2PSessionBuilder::<StubConfig>::new(2, socket)
        .add_player(PlayerType::Local, 0)?
        .add_player(PlayerType::Remote(remote_addr), 1)?
        .add_player(PlayerType::Spectator(spec_addr), 2)?
        .start_session()?;

    assert!(sess.disconnect_player(5).is_err()); // invalid handle
    assert!(sess.disconnect_player(0).is_err()); // for now, local players cannot be disconnected
    assert!(sess.disconnect_player(1).is_ok());
    assert!(sess.disconnect_player(1).is_err()); // already disconnected
    assert!(sess.disconnect_player(2).is_ok());

    Ok(())
}

#[test]
#[serial]
fn test_synchronize_p2p_sessions() -> Result<(), GGRSError> {
    let addr1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 7777);
    let addr2 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8888);

    let socket1 = UdpNonBlockingSocket::bind_to_port(7777).unwrap();
    let mut sess1 = P2PSessionBuilder::<StubConfig>::new(2, socket1)
        .add_player(PlayerType::Local, 0)?
        .add_player(PlayerType::Remote(addr2), 1)?
        .start_session()?;

    let socket2 = UdpNonBlockingSocket::bind_to_port(8888).unwrap();
    let mut sess2 = P2PSessionBuilder::<StubConfig>::new(2, socket2)
        .add_player(PlayerType::Local, 1)?
        .add_player(PlayerType::Remote(addr1), 0)?
        .start_session()?;

    assert!(sess1.current_state() == SessionState::Synchronizing);
    assert!(sess2.current_state() == SessionState::Synchronizing);

    for _ in 0..10 {
        sess1.poll_remote_clients();
        sess2.poll_remote_clients();
    }

    assert!(sess1.current_state() == SessionState::Running);
    assert!(sess2.current_state() == SessionState::Running);

    Ok(())
}

#[test]
#[serial]
fn test_advance_frame_p2p_sessions() -> Result<(), GGRSError> {
    let addr1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 7777);
    let addr2 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8888);

    let socket1 = UdpNonBlockingSocket::bind_to_port(7777).unwrap();
    let mut sess1 = P2PSessionBuilder::<StubConfig>::new(2, socket1)
        .add_player(PlayerType::Local, 0)?
        .add_player(PlayerType::Remote(addr2), 1)?
        .start_session()?;

    let socket2 = UdpNonBlockingSocket::bind_to_port(8888).unwrap();
    let mut sess2 = P2PSessionBuilder::<StubConfig>::new(2, socket2)
        .add_player(PlayerType::Remote(addr1), 0)?
        .add_player(PlayerType::Local, 1)?
        .start_session()?;

    assert!(sess1.current_state() == SessionState::Synchronizing);
    assert!(sess2.current_state() == SessionState::Synchronizing);

    for _ in 0..10 {
        sess1.poll_remote_clients();
        sess2.poll_remote_clients();
    }

    assert!(sess1.current_state() == SessionState::Running);
    assert!(sess2.current_state() == SessionState::Running);

    let mut stub1 = stubs::GameStub::new();
    let mut stub2 = stubs::GameStub::new();
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

    Ok(())
}
