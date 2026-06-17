mod stubs;

use ggrs::{
    GgrsError, GgrsRequest, PlayerType, SessionBuilder, SessionState, SpectatorSession,
    UdpNonBlockingSocket,
};
use serial_test::serial;
use std::thread;
use std::time::{Duration, Instant};
use stubs::{StubConfig, StubInput};

const TEST_TIMEOUT: Duration = Duration::from_secs(2);
const POLL_INTERVAL: Duration = Duration::from_millis(5);

fn advance_spectator_when_ready(
    spec_sess: &mut SpectatorSession<StubConfig>,
) -> Result<Vec<GgrsRequest<StubConfig>>, GgrsError> {
    let deadline = Instant::now() + TEST_TIMEOUT;
    loop {
        match spec_sess.advance_frame() {
            Ok(requests) => return Ok(requests),
            Err(GgrsError::PredictionThreshold) if Instant::now() < deadline => {
                thread::sleep(POLL_INTERVAL);
            }
            Err(e) => return Err(e),
        }
    }
}

#[test]
#[serial]
fn test_start_session() {
    let socket = UdpNonBlockingSocket::bind_to_port(7800).unwrap();
    let spec_sess =
        SessionBuilder::<StubConfig>::new().start_spectator_session(stubs::localhost(7801), socket);
    assert!(spec_sess.current_state() == SessionState::Synchronizing);
}

#[test]
#[serial]
fn test_synchronize_with_host() -> Result<(), GgrsError> {
    let socket1 = UdpNonBlockingSocket::bind_to_port(7802).unwrap();
    let mut host_sess = SessionBuilder::<StubConfig>::new()
        .with_num_players(1)?
        .add_player(PlayerType::Local, 0)?
        .add_player(PlayerType::Spectator(stubs::localhost(7803)), 2)?
        .start_p2p_session(socket1)?;

    let socket2 = UdpNonBlockingSocket::bind_to_port(7803).unwrap();
    let mut spec_sess = SessionBuilder::<StubConfig>::new()
        .start_spectator_session(stubs::localhost(7802), socket2);

    assert_eq!(spec_sess.current_state(), SessionState::Synchronizing);
    assert_eq!(host_sess.current_state(), SessionState::Synchronizing);

    stubs::sync_host_and_spectator(&mut host_sess, &mut spec_sess);

    assert_eq!(spec_sess.current_state(), SessionState::Running);
    assert_eq!(host_sess.current_state(), SessionState::Running);

    Ok(())
}

#[test]
#[serial]
fn test_spectator_observes_frames() -> Result<(), GgrsError> {
    let (mut host_sess, mut spec_sess) = stubs::make_host_and_spectator(7804, 7805)?;
    let mut host_stub = stubs::GameStub1P::new();
    let mut spec_stub = stubs::GameStub1P::new();

    // confirmed_frame is computed before local inputs are registered in the same advance_frame
    // call, so inputs for frame N are only confirmed (and sent to the spectator) during frame N+1.
    // We drive 11 host frames and expect the spectator to be able to observe 10 of them.
    for i in 0..11 {
        host_sess.add_local_input(0, StubInput { inp: 1 }).unwrap();
        let host_requests = host_sess.advance_frame().unwrap();
        host_stub.handle_requests(host_requests);

        if i > 0 {
            // inputs for frame i-1 are now confirmed and have been sent; spectator can advance
            let spec_requests = advance_spectator_when_ready(&mut spec_sess)?;
            assert!(
                spec_requests
                    .iter()
                    .any(|r| matches!(r, GgrsRequest::AdvanceFrame { .. })),
                "spectator should have received an AdvanceFrame request at iteration {i}"
            );
            spec_stub.handle_requests(spec_requests);
        }
    }

    assert_eq!(host_stub.gs.frame, 11);
    assert_eq!(spec_stub.gs.frame, 10);

    Ok(())
}

#[test]
#[serial]
fn test_spectator_catches_up_after_lag() -> Result<(), GgrsError> {
    let mut host_sess = SessionBuilder::<StubConfig>::new()
        .with_num_players(1)?
        .add_player(PlayerType::Local, 0)?
        .add_player(PlayerType::Spectator(stubs::localhost(7807)), 1)?
        .start_p2p_session(UdpNonBlockingSocket::bind_to_port(7806).unwrap())?;

    // catchup_speed=2 frames per advance_frame call when more than max_frames_behind=4 frames behind
    let mut spec_sess = SessionBuilder::<StubConfig>::new()
        .with_num_players(1)?
        .with_max_frames_behind(4)?
        .with_catchup_speed(2)?
        .start_spectator_session(
            stubs::localhost(7806),
            UdpNonBlockingSocket::bind_to_port(7807).unwrap(),
        );

    stubs::sync_host_and_spectator(&mut host_sess, &mut spec_sess);
    assert_eq!(spec_sess.current_state(), SessionState::Running);

    let mut host_stub = stubs::GameStub1P::new();

    // confirmed_frame is computed before local inputs are registered, so inputs for frame N are
    // only confirmed (and sent to the spectator) during frame N+1. Drive the host until the
    // spectator has definitely received enough confirmed frames to enter catch-up mode.
    let deadline = Instant::now() + TEST_TIMEOUT;
    while spec_sess.frames_behind_host() <= 4 && Instant::now() < deadline {
        host_sess.add_local_input(0, StubInput { inp: 1 }).unwrap();
        let requests = host_sess.advance_frame().unwrap();
        host_stub.handle_requests(requests);
        spec_sess.poll_remote_clients(); // receive packets (but don't advance)
        thread::sleep(POLL_INTERVAL);
    }

    // spectator should now be more than max_frames_behind=4 frames behind the host
    assert!(
        spec_sess.frames_behind_host() > 4,
        "expected >4 behind, got {}",
        spec_sess.frames_behind_host()
    );

    // one advance_frame call should advance 2 frames (catchup_speed=2)
    let mut spec_stub = stubs::GameStub1P::new();
    let requests = advance_spectator_when_ready(&mut spec_sess)?;
    let advance_count = requests
        .iter()
        .filter(|r| matches!(r, GgrsRequest::AdvanceFrame { .. }))
        .count();
    assert_eq!(
        advance_count, 2,
        "spectator should catch up by 2 frames per step"
    );
    spec_stub.handle_requests(requests);
    assert_eq!(spec_stub.gs.frame, 2);

    Ok(())
}

#[test]
#[serial]
fn test_spectator_caps_catchup_speed_to_available_frames() -> Result<(), GgrsError> {
    let mut host_sess = SessionBuilder::<StubConfig>::new()
        .with_num_players(1)?
        .add_player(PlayerType::Local, 0)?
        .add_player(PlayerType::Spectator(stubs::localhost(7809)), 1)?
        .start_p2p_session(UdpNonBlockingSocket::bind_to_port(7808).unwrap())?;

    let mut spec_sess = SessionBuilder::<StubConfig>::new()
        .with_num_players(1)?
        .with_max_frames_behind(4)?
        .with_catchup_speed(10)?
        .start_spectator_session(
            stubs::localhost(7808),
            UdpNonBlockingSocket::bind_to_port(7809).unwrap(),
        );

    stubs::sync_host_and_spectator(&mut host_sess, &mut spec_sess);
    assert_eq!(spec_sess.current_state(), SessionState::Running);

    let mut host_stub = stubs::GameStub1P::new();
    let deadline = Instant::now() + TEST_TIMEOUT;
    while spec_sess.frames_behind_host() <= 4 && Instant::now() < deadline {
        host_sess.add_local_input(0, StubInput { inp: 1 }).unwrap();
        let requests = host_sess.advance_frame().unwrap();
        host_stub.handle_requests(requests);
        spec_sess.poll_remote_clients();
        thread::sleep(POLL_INTERVAL);
    }

    let frames_available = spec_sess.frames_behind_host();
    assert!(
        frames_available > 4,
        "expected >4 frames available, got {frames_available}"
    );

    let mut spec_stub = stubs::GameStub1P::new();
    let requests = advance_spectator_when_ready(&mut spec_sess)?;
    let advance_count = requests
        .iter()
        .filter(|r| matches!(r, GgrsRequest::AdvanceFrame { .. }))
        .count();

    assert!(advance_count >= frames_available);
    assert!(advance_count <= 10);
    spec_stub.handle_requests(requests);
    assert_eq!(spec_stub.gs.frame, advance_count as i32);
    assert_eq!(spec_sess.frames_behind_host(), 0);

    Ok(())
}
