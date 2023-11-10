mod stubs;

use ggrs::{GgrsError, PlayerType, SessionBuilder, SessionState, UdpNonBlockingSocket};
use serial_test::serial;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use stubs::StubConfig;

#[test]
#[serial]
fn test_start_session() {
    let host_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 7777);
    let socket = UdpNonBlockingSocket::bind_to_port(9999).unwrap();
    let spec_sess = SessionBuilder::<StubConfig>::new().start_spectator_session(host_addr, socket);
    assert!(spec_sess.current_state() == SessionState::Synchronizing);
}

#[test]
#[serial]
fn test_synchronize_with_host() -> Result<(), GgrsError> {
    let host_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 7777);
    let spec_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8888);

    let socket1 = UdpNonBlockingSocket::bind_to_port(7777).unwrap();
    let mut host_sess = SessionBuilder::<StubConfig>::new()
        .with_num_players(1)
        .add_player(PlayerType::Local, 0)?
        .add_player(PlayerType::Spectator(spec_addr), 2)?
        .start_p2p_session(socket1)?;

    let socket2 = UdpNonBlockingSocket::bind_to_port(8888).unwrap();
    let mut spec_sess =
        SessionBuilder::<StubConfig>::new().start_spectator_session(host_addr, socket2);

    assert_eq!(spec_sess.current_state(), SessionState::Synchronizing);
    assert_eq!(host_sess.current_state(), SessionState::Synchronizing);

    for _ in 0..50 {
        spec_sess.poll_remote_clients();
        host_sess.poll_remote_clients();
    }

    assert_eq!(spec_sess.current_state(), SessionState::Running);
    assert_eq!(host_sess.current_state(), SessionState::Running);

    Ok(())
}
