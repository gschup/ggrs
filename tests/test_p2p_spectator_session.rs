mod stubs;

use ggrs::{P2PSession, P2PSpectatorSession, PlayerType, SessionState, UdpNonBlockingSocket};
use serial_test::serial;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use stubs::StubConfig;

#[test]
#[serial]
fn test_create_session() {
    let host_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 7777);
    let socket = UdpNonBlockingSocket::bind_to_port(9999).unwrap();
    let _sess = P2PSpectatorSession::<StubConfig>::new(1, socket, host_addr);
}

#[test]
#[serial]
fn test_start_session() {
    let host_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 7777);
    let socket = UdpNonBlockingSocket::bind_to_port(9999).unwrap();
    let mut spec_sess = P2PSpectatorSession::<StubConfig>::new(1, socket, host_addr);
    assert!(spec_sess.start_session().is_ok());
    assert!(spec_sess.current_state() == SessionState::Synchronizing);
}

#[test]
#[serial]
fn test_synchronize_with_host() {
    let host_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 7777);
    let spec_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8888);

    let socket1 = UdpNonBlockingSocket::bind_to_port(7777).unwrap();
    let mut host_sess = P2PSession::<StubConfig>::new(1, stubs::MAX_PRED_FRAMES, socket1);
    let socket2 = UdpNonBlockingSocket::bind_to_port(8888).unwrap();
    let mut spec_sess = P2PSpectatorSession::<StubConfig>::new(1, socket2, host_addr);

    host_sess.add_player(PlayerType::Local, 0).unwrap();
    host_sess
        .add_player(PlayerType::Spectator(spec_addr), 2)
        .unwrap();

    host_sess.start_session().unwrap();
    spec_sess.start_session().unwrap();

    assert_eq!(spec_sess.current_state(), SessionState::Synchronizing);
    assert_eq!(host_sess.current_state(), SessionState::Synchronizing);

    for _ in 0..10 {
        spec_sess.poll_remote_clients();
        host_sess.poll_remote_clients();
    }

    assert_eq!(spec_sess.current_state(), SessionState::Running);
    assert_eq!(host_sess.current_state(), SessionState::Running);
}
