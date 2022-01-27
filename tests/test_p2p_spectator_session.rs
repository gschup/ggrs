mod stubs;

use ggrs::{
    P2PSessionBuilder, PlayerType, SessionState, SpectatorSessionBuilder, UdpNonBlockingSocket,
};
use serial_test::serial;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use stubs::StubConfig;

#[test]
#[serial]
fn test_start_session() {
    let host_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 7777);
    let socket = UdpNonBlockingSocket::bind_to_port(9999).unwrap();
    let spec_sess =
        SpectatorSessionBuilder::<StubConfig>::new(1, socket, host_addr).start_session();
    assert!(spec_sess.current_state() == SessionState::Synchronizing);
}

#[test]
#[serial]
fn test_synchronize_with_host() {
    let host_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 7777);
    let spec_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8888);

    let socket1 = UdpNonBlockingSocket::bind_to_port(7777).unwrap();
    let mut host_sess = P2PSessionBuilder::<StubConfig>::new(1, socket1)
        .add_player(PlayerType::Local, 0)
        .unwrap()
        .add_player(PlayerType::Spectator(spec_addr), 2)
        .unwrap()
        .start_session()
        .unwrap();

    let socket2 = UdpNonBlockingSocket::bind_to_port(8888).unwrap();
    let mut spec_sess =
        SpectatorSessionBuilder::<StubConfig>::new(1, socket2, host_addr).start_session();

    assert_eq!(spec_sess.current_state(), SessionState::Synchronizing);
    assert_eq!(host_sess.current_state(), SessionState::Synchronizing);

    for _ in 0..10 {
        spec_sess.poll_remote_clients();
        host_sess.poll_remote_clients();
    }

    assert_eq!(spec_sess.current_state(), SessionState::Running);
    assert_eq!(host_sess.current_state(), SessionState::Running);
}
