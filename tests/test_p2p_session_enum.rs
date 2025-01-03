mod stubs_enum;

use ggrs::{GgrsError, PlayerType, SessionBuilder, SessionState, UdpNonBlockingSocket};
use serial_test::serial;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use stubs_enum::{EnumInput, GameStubEnum, StubEnumConfig};

#[test]
#[serial]
fn test_advance_frame_p2p_sessions_enum() -> Result<(), GgrsError> {
    let addr1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 7777);
    let addr2 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8888);

    let socket1 = UdpNonBlockingSocket::bind_to_port(7777).unwrap();
    let mut sess1 = SessionBuilder::<StubEnumConfig>::new()
        .add_player(PlayerType::Local, 0)?
        .add_player(PlayerType::Remote(addr2), 1)?
        .start_p2p_session(socket1)?;

    let socket2 = UdpNonBlockingSocket::bind_to_port(8888).unwrap();
    let mut sess2 = SessionBuilder::<StubEnumConfig>::new()
        .add_player(PlayerType::Remote(addr1), 0)?
        .add_player(PlayerType::Local, 1)?
        .start_p2p_session(socket2)?;

    assert!(sess1.current_state() == SessionState::Synchronizing);
    assert!(sess2.current_state() == SessionState::Synchronizing);

    for _ in 0..50 {
        sess1.poll_remote_clients();
        sess2.poll_remote_clients();
    }

    assert!(sess1.current_state() == SessionState::Running);
    assert!(sess2.current_state() == SessionState::Running);

    let mut stub1 = GameStubEnum::new();
    let mut stub2 = GameStubEnum::new();
    let reps = 10;
    for i in 0..reps {
        sess1.poll_remote_clients();
        sess2.poll_remote_clients();

        sess1
            .add_local_input(
                0,
                if i % 2 == 0 {
                    EnumInput::Val1
                } else {
                    EnumInput::Val2
                },
            )
            .unwrap();
        let requests1 = sess1.advance_frame().unwrap();
        stub1.handle_requests(requests1);
        sess2
            .add_local_input(
                1,
                if i % 3 == 0 {
                    EnumInput::Val1
                } else {
                    EnumInput::Val2
                },
            )
            .unwrap();
        let requests2 = sess2.advance_frame().unwrap();
        stub2.handle_requests(requests2);

        // gamestate evolves
        assert_eq!(stub1.gs.frame, i as i32 + 1);
        assert_eq!(stub2.gs.frame, i as i32 + 1);
    }

    Ok(())
}
