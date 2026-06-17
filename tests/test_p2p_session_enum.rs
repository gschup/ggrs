mod stubs_enum;

use ggrs::{GgrsError, P2PSession, PlayerType, SessionBuilder, SessionState, UdpNonBlockingSocket};
use serial_test::serial;
use std::thread;
use std::time::{Duration, Instant};
use stubs_enum::{EnumInput, GameStubEnum, StubEnumConfig};

const SYNC_TIMEOUT: Duration = Duration::from_secs(2);
const SYNC_POLL_INTERVAL: Duration = Duration::from_millis(5);

fn sync_p2p_sessions(s1: &mut P2PSession<StubEnumConfig>, s2: &mut P2PSession<StubEnumConfig>) {
    let deadline = Instant::now() + SYNC_TIMEOUT;
    while Instant::now() < deadline {
        s1.poll_remote_clients();
        s2.poll_remote_clients();
        if s1.current_state() == SessionState::Running
            && s2.current_state() == SessionState::Running
        {
            return;
        }
        thread::sleep(SYNC_POLL_INTERVAL);
    }
    assert!(s1.current_state() == SessionState::Running);
    assert!(s2.current_state() == SessionState::Running);
}

#[test]
#[serial]
fn test_advance_frame_p2p_sessions_enum() -> Result<(), GgrsError> {
    let addr1 = stubs_enum::localhost(7860);
    let addr2 = stubs_enum::localhost(7861);

    let socket1 = UdpNonBlockingSocket::bind_to_port(7860).unwrap();
    let mut sess1 = SessionBuilder::<StubEnumConfig>::new()
        .add_player(PlayerType::Local, 0)?
        .add_player(PlayerType::Remote(addr2), 1)?
        .start_p2p_session(socket1)?;

    let socket2 = UdpNonBlockingSocket::bind_to_port(7861).unwrap();
    let mut sess2 = SessionBuilder::<StubEnumConfig>::new()
        .add_player(PlayerType::Remote(addr1), 0)?
        .add_player(PlayerType::Local, 1)?
        .start_p2p_session(socket2)?;

    assert!(sess1.current_state() == SessionState::Synchronizing);
    assert!(sess2.current_state() == SessionState::Synchronizing);

    sync_p2p_sessions(&mut sess1, &mut sess2);

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
        assert_eq!(stub1.gs.frame, i + 1);
        assert_eq!(stub2.gs.frame, i + 1);
    }

    Ok(())
}
