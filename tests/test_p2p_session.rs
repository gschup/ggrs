mod stubs;

use ggrs::{
    DesyncDetection, GgrsError, GgrsEvent, PlayerType, SessionBuilder, SessionState,
    UdpNonBlockingSocket,
};
use serial_test::serial;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use stubs::{StubConfig, StubInput};

fn make_session(
    port: u16,
    local: u16,
    remote: u16,
) -> (ggrs::P2PSession<StubConfig>, ggrs::P2PSession<StubConfig>) {
    let addr1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), port);
    let addr2 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), remote);

    let s1 = SessionBuilder::<StubConfig>::new()
        .add_player(PlayerType::Local, 0)
        .unwrap()
        .add_player(PlayerType::Remote(addr2), 1)
        .unwrap()
        .start_p2p_session(UdpNonBlockingSocket::bind_to_port(local).unwrap())
        .unwrap();
    let s2 = SessionBuilder::<StubConfig>::new()
        .add_player(PlayerType::Remote(addr1), 0)
        .unwrap()
        .add_player(PlayerType::Local, 1)
        .unwrap()
        .start_p2p_session(UdpNonBlockingSocket::bind_to_port(remote).unwrap())
        .unwrap();
    (s1, s2)
}

fn sync_sessions(s1: &mut ggrs::P2PSession<StubConfig>, s2: &mut ggrs::P2PSession<StubConfig>) {
    for _ in 0..50 {
        s1.poll_remote_clients();
        s2.poll_remote_clients();
    }
    assert_eq!(s1.current_state(), SessionState::Running);
    assert_eq!(s2.current_state(), SessionState::Running);
}

#[test]
#[serial]
fn test_add_more_players() -> Result<(), GgrsError> {
    let socket = UdpNonBlockingSocket::bind_to_port(7777).unwrap();
    let remote_addr1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
    let remote_addr2 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081);
    let remote_addr3 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8082);
    let spec_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8090);

    let _sess = SessionBuilder::<StubConfig>::new()
        .with_num_players(4)
        .add_player(PlayerType::Local, 0)?
        .add_player(PlayerType::Remote(remote_addr1), 1)?
        .add_player(PlayerType::Remote(remote_addr2), 2)?
        .add_player(PlayerType::Remote(remote_addr3), 3)?
        .add_player(PlayerType::Spectator(spec_addr), 4)?
        .start_p2p_session(socket)?;
    Ok(())
}

#[test]
#[serial]
fn test_start_session() -> Result<(), GgrsError> {
    let socket = UdpNonBlockingSocket::bind_to_port(7777).unwrap();
    let remote_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
    let spec_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8090);

    let _sess = SessionBuilder::<StubConfig>::new()
        .add_player(PlayerType::Local, 0)?
        .add_player(PlayerType::Remote(remote_addr), 1)?
        .add_player(PlayerType::Spectator(spec_addr), 2)?
        .start_p2p_session(socket)?;
    Ok(())
}

#[test]
#[serial]
fn test_disconnect_player() -> Result<(), GgrsError> {
    let socket = UdpNonBlockingSocket::bind_to_port(7777).unwrap();
    let remote_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
    let spec_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8090);

    let mut sess = SessionBuilder::<StubConfig>::new()
        .add_player(PlayerType::Local, 0)?
        .add_player(PlayerType::Remote(remote_addr), 1)?
        .add_player(PlayerType::Spectator(spec_addr), 2)?
        .start_p2p_session(socket)?;

    assert!(sess.disconnect_player(5).is_err()); // invalid handle
    assert!(sess.disconnect_player(0).is_err()); // for now, local players cannot be disconnected
    assert!(sess.disconnect_player(1).is_ok());
    assert!(sess.disconnect_player(1).is_err()); // already disconnected
    assert!(sess.disconnect_player(2).is_ok());

    Ok(())
}

#[test]
#[serial]
fn test_synchronize_p2p_sessions() -> Result<(), GgrsError> {
    let addr1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 7777);
    let addr2 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8888);

    let socket1 = UdpNonBlockingSocket::bind_to_port(7777).unwrap();
    let mut sess1 = SessionBuilder::<StubConfig>::new()
        .add_player(PlayerType::Local, 0)?
        .add_player(PlayerType::Remote(addr2), 1)?
        .start_p2p_session(socket1)?;

    let socket2 = UdpNonBlockingSocket::bind_to_port(8888).unwrap();
    let mut sess2 = SessionBuilder::<StubConfig>::new()
        .add_player(PlayerType::Local, 1)?
        .add_player(PlayerType::Remote(addr1), 0)?
        .start_p2p_session(socket2)?;

    assert!(sess1.current_state() == SessionState::Synchronizing);
    assert!(sess2.current_state() == SessionState::Synchronizing);

    for _ in 0..50 {
        sess1.poll_remote_clients();
        sess2.poll_remote_clients();
    }

    assert!(sess1.current_state() == SessionState::Running);
    assert!(sess2.current_state() == SessionState::Running);

    Ok(())
}

#[test]
#[serial]
fn test_advance_frame_p2p_sessions() -> Result<(), GgrsError> {
    let addr1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 7777);
    let addr2 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8888);

    let socket1 = UdpNonBlockingSocket::bind_to_port(7777).unwrap();
    let mut sess1 = SessionBuilder::<StubConfig>::new()
        .add_player(PlayerType::Local, 0)?
        .add_player(PlayerType::Remote(addr2), 1)?
        .start_p2p_session(socket1)?;

    let socket2 = UdpNonBlockingSocket::bind_to_port(8888).unwrap();
    let mut sess2 = SessionBuilder::<StubConfig>::new()
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

    let mut stub1 = stubs::GameStub::new();
    let mut stub2 = stubs::GameStub::new();
    let reps = 10;
    for i in 0..reps {
        sess1.poll_remote_clients();
        sess2.poll_remote_clients();

        sess1.add_local_input(0, StubInput { inp: i }).unwrap();
        let requests1 = sess1.advance_frame().unwrap();
        stub1.handle_requests(requests1);
        sess2.add_local_input(1, StubInput { inp: i }).unwrap();
        let requests2 = sess2.advance_frame().unwrap();
        stub2.handle_requests(requests2);

        // gamestate evolves
        assert_eq!(stub1.gs.frame, i as i32 + 1);
        assert_eq!(stub2.gs.frame, i as i32 + 1);
    }

    Ok(())
}

#[test]
#[serial]
fn test_desyncs_detected() -> Result<(), GgrsError> {
    let addr1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 7777);
    let addr2 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8888);
    let desync_mode = DesyncDetection::On { interval: 100 };

    let socket1 = UdpNonBlockingSocket::bind_to_port(7777).unwrap();
    let mut sess1 = SessionBuilder::<StubConfig>::new()
        .add_player(PlayerType::Local, 0)?
        .add_player(PlayerType::Remote(addr2), 1)?
        .with_desync_detection_mode(desync_mode)
        .start_p2p_session(socket1)?;

    let socket2 = UdpNonBlockingSocket::bind_to_port(8888).unwrap();
    let mut sess2 = SessionBuilder::<StubConfig>::new()
        .add_player(PlayerType::Remote(addr1), 0)?
        .add_player(PlayerType::Local, 1)?
        .with_desync_detection_mode(desync_mode)
        .start_p2p_session(socket2)?;

    while sess1.current_state() != SessionState::Running
        && sess2.current_state() != SessionState::Running
    {
        sess1.poll_remote_clients();
        sess2.poll_remote_clients();
    }

    // drain events
    assert!(sess1.events().chain(sess2.events()).all(|e| match e {
        GgrsEvent::Synchronizing { .. } | GgrsEvent::Synchronized { .. } => true,
        _ => false,
    }));

    let mut stub1 = stubs::GameStub::new();
    let mut stub2 = stubs::GameStub::new();

    // run normally for some frames (past first desync interval)
    for i in 0..110 {
        sess1.poll_remote_clients();
        sess2.poll_remote_clients();

        sess1.add_local_input(0, StubInput { inp: i }).unwrap();
        sess2.add_local_input(1, StubInput { inp: i }).unwrap();

        let requests1 = sess1.advance_frame().unwrap();
        let requests2 = sess2.advance_frame().unwrap();

        stub1.handle_requests(requests1);
        stub2.handle_requests(requests2);
    }

    // check that there are no unexpected events yet
    assert_eq!(sess1.events().len(), 0);
    assert_eq!(sess2.events().len(), 0);

    // run for some more frames
    for _ in 0..100 {
        sess1.poll_remote_clients();
        sess2.poll_remote_clients();

        // mess up state for peer 1
        stub1.gs.state = 1234;

        // keep input steady (to avoid loads, which would restore valid state)
        sess1.add_local_input(0, StubInput { inp: 0 }).unwrap();
        sess2.add_local_input(1, StubInput { inp: 1 }).unwrap();

        let requests1 = sess1.advance_frame().unwrap();
        let requests2 = sess2.advance_frame().unwrap();

        stub1.handle_requests(requests1);
        stub2.handle_requests(requests2);
    }

    // check that we got desync events
    let sess1_events: Vec<_> = sess1.events().collect();
    let sess2_events: Vec<_> = sess2.events().collect();
    assert_eq!(sess1_events.len(), 1);
    assert_eq!(sess2_events.len(), 1);

    let GgrsEvent::DesyncDetected {
        frame: desync_frame1,
        local_checksum: desync_local_checksum1,
        remote_checksum: desync_remote_checksum1,
        addr: desync_addr1,
    } = sess1_events[0]
    else {
        panic!("no desync for peer 1");
    };
    assert_eq!(desync_frame1, 200);
    assert_eq!(desync_addr1, addr2);
    assert_ne!(desync_local_checksum1, desync_remote_checksum1);

    let GgrsEvent::DesyncDetected {
        frame: desync_frame2,
        local_checksum: desync_local_checksum2,
        remote_checksum: desync_remote_checksum2,
        addr: desync_addr2,
    } = sess2_events[0]
    else {
        panic!("no desync for peer 2");
    };
    assert_eq!(desync_frame2, 200);
    assert_eq!(desync_addr2, addr1);
    assert_ne!(desync_local_checksum2, desync_remote_checksum2);

    // check that checksums match
    assert_eq!(desync_remote_checksum1, desync_local_checksum2);
    assert_eq!(desync_remote_checksum2, desync_local_checksum1);

    Ok(())
}

#[test]
#[serial]
fn test_desyncs_and_input_delay_no_panic() -> Result<(), GgrsError> {
    let addr1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 7777);
    let addr2 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8888);
    let desync_mode = DesyncDetection::On { interval: 100 };

    let socket1 = UdpNonBlockingSocket::bind_to_port(7777).unwrap();
    let mut sess1 = SessionBuilder::<StubConfig>::new()
        .add_player(PlayerType::Local, 0)?
        .add_player(PlayerType::Remote(addr2), 1)?
        .with_input_delay(5)
        .with_desync_detection_mode(desync_mode)
        .start_p2p_session(socket1)?;

    let socket2 = UdpNonBlockingSocket::bind_to_port(8888).unwrap();
    let mut sess2 = SessionBuilder::<StubConfig>::new()
        .add_player(PlayerType::Remote(addr1), 0)?
        .add_player(PlayerType::Local, 1)?
        .with_input_delay(5)
        .with_desync_detection_mode(desync_mode)
        .start_p2p_session(socket2)?;

    while sess1.current_state() != SessionState::Running
        && sess2.current_state() != SessionState::Running
    {
        sess1.poll_remote_clients();
        sess2.poll_remote_clients();
    }

    // drain events
    assert!(sess1.events().chain(sess2.events()).all(|e| match e {
        GgrsEvent::Synchronizing { .. } | GgrsEvent::Synchronized { .. } => true,
        _ => false,
    }));

    let mut stub1 = stubs::GameStub::new();
    let mut stub2 = stubs::GameStub::new();

    // run normally for some frames (past first desync interval)
    for i in 0..150 {
        sess1.poll_remote_clients();
        sess2.poll_remote_clients();

        sess1.add_local_input(0, StubInput { inp: i }).unwrap();
        sess2.add_local_input(1, StubInput { inp: i }).unwrap();

        let requests1 = sess1.advance_frame().unwrap();
        let requests2 = sess2.advance_frame().unwrap();

        stub1.handle_requests(requests1);
        stub2.handle_requests(requests2);
    }

    Ok(())
}

// ── Builder validation ────────────────────────────────────────────────────────

#[test]
fn test_builder_duplicate_player_handle_errors() {
    let remote_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
    let result = SessionBuilder::<StubConfig>::new()
        .add_player(PlayerType::Local, 0)
        .unwrap()
        .add_player(PlayerType::Remote(remote_addr), 0); // duplicate handle
    assert!(result.is_err());
}

#[test]
fn test_builder_local_handle_out_of_range_errors() {
    // handle 5 >= num_players(2) for a Local player → error
    let result = SessionBuilder::<StubConfig>::new().add_player(PlayerType::Local, 5);
    assert!(result.is_err());
}

#[test]
fn test_builder_remote_handle_out_of_range_errors() {
    let remote_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
    // handle 5 >= num_players(2) for a Remote player → error
    let result = SessionBuilder::<StubConfig>::new().add_player(PlayerType::Remote(remote_addr), 5);
    assert!(result.is_err());
}

#[test]
fn test_builder_spectator_handle_below_num_players_errors() {
    let spec_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 9000);
    // handle 0 < num_players(2) for a Spectator → error
    let result =
        SessionBuilder::<StubConfig>::new().add_player(PlayerType::Spectator(spec_addr), 0);
    assert!(result.is_err());
}

#[test]
#[serial]
fn test_builder_missing_player_errors() {
    let socket = UdpNonBlockingSocket::bind_to_port(7777).unwrap();
    // num_players=2 but only player 0 registered
    let result = SessionBuilder::<StubConfig>::new()
        .add_player(PlayerType::Local, 0)
        .unwrap()
        .start_p2p_session(socket);
    assert!(result.is_err());
}

#[test]
fn test_builder_fps_zero_errors() {
    let result = SessionBuilder::<StubConfig>::new().with_fps(0);
    assert!(result.is_err());
}

// ── Session behaviour ─────────────────────────────────────────────────────────

#[test]
#[serial]
fn test_synchronizing_events_emitted() -> Result<(), GgrsError> {
    let (mut sess1, mut sess2) = make_session(7777, 7777, 8888);
    sync_sessions(&mut sess1, &mut sess2);

    let events1: Vec<_> = sess1.events().collect();
    assert!(events1
        .iter()
        .any(|e| matches!(e, GgrsEvent::Synchronizing { .. })));
    assert!(events1
        .iter()
        .any(|e| matches!(e, GgrsEvent::Synchronized { .. })));

    let events2: Vec<_> = sess2.events().collect();
    assert!(events2
        .iter()
        .any(|e| matches!(e, GgrsEvent::Synchronizing { .. })));
    assert!(events2
        .iter()
        .any(|e| matches!(e, GgrsEvent::Synchronized { .. })));

    Ok(())
}

#[test]
#[serial]
fn test_network_stats_invalid_handles() -> Result<(), GgrsError> {
    let (mut sess1, mut sess2) = make_session(7777, 7777, 8888);
    sync_sessions(&mut sess1, &mut sess2);

    // invalid handle → error
    assert!(sess1.network_stats(99).is_err());
    // local player handle → error (no network stats for local players)
    assert!(sess1.network_stats(0).is_err());
    assert!(sess2.network_stats(1).is_err());

    Ok(())
}

#[test]
#[serial]
fn test_network_stats_spectator_handle_does_not_panic() -> Result<(), GgrsError> {
    let addr1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 7777);
    let spec_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 9999);

    let socket = UdpNonBlockingSocket::bind_to_port(7777).unwrap();
    let sess = SessionBuilder::<StubConfig>::new()
        .add_player(PlayerType::Local, 0)?
        .add_player(PlayerType::Remote(addr1), 1)?
        .add_player(PlayerType::Spectator(spec_addr), 2)?
        .start_p2p_session(socket)?;

    // spectator handle must not panic — previously looked up addr in the wrong map
    let result = sess.network_stats(2);
    assert!(result.is_err()); // NotSynchronized is fine; a panic is not

    Ok(())
}

#[test]
#[serial]
fn test_game_state_converges() -> Result<(), GgrsError> {
    let (mut sess1, mut sess2) = make_session(7777, 7777, 8888);
    sync_sessions(&mut sess1, &mut sess2);

    let mut stub1 = stubs::GameStub::new();
    let mut stub2 = stubs::GameStub::new();

    // use constant input so predictions always match — no mispredictions, fully deterministic
    for _ in 0..50 {
        sess1.poll_remote_clients();
        sess2.poll_remote_clients();

        sess1.add_local_input(0, StubInput { inp: 0 }).unwrap();
        let requests1 = sess1.advance_frame().unwrap();
        stub1.handle_requests(requests1);

        sess2.add_local_input(1, StubInput { inp: 0 }).unwrap();
        let requests2 = sess2.advance_frame().unwrap();
        stub2.handle_requests(requests2);
    }

    // both sessions must reach the same game state
    assert_eq!(stub1.gs.state, stub2.gs.state);
    assert_eq!(stub1.gs.frame, stub2.gs.frame);

    Ok(())
}

#[test]
#[serial]
fn test_desync_detection_off_by_default() -> Result<(), GgrsError> {
    // Sessions created without calling with_desync_detection_mode → Off by default
    let (mut sess1, mut sess2) = make_session(7777, 7777, 8888);
    sync_sessions(&mut sess1, &mut sess2);

    // drain sync events
    let _ = sess1.events().count();
    let _ = sess2.events().count();

    let mut stub1 = stubs::GameStub::new();
    let mut stub2 = stubs::GameStub::new();

    for _ in 0..100 {
        sess1.poll_remote_clients();
        sess2.poll_remote_clients();

        // corrupt sess1 state so checksums diverge
        stub1.gs.state = 9999;

        sess1.add_local_input(0, StubInput { inp: 0 }).unwrap();
        sess2.add_local_input(1, StubInput { inp: 0 }).unwrap();

        let requests1 = sess1.advance_frame().unwrap();
        let requests2 = sess2.advance_frame().unwrap();

        stub1.handle_requests(requests1);
        stub2.handle_requests(requests2);
    }

    // with detection off, no DesyncDetected events should have been emitted
    let events1: Vec<_> = sess1.events().collect();
    let events2: Vec<_> = sess2.events().collect();
    assert!(!events1
        .iter()
        .any(|e| matches!(e, GgrsEvent::DesyncDetected { .. })));
    assert!(!events2
        .iter()
        .any(|e| matches!(e, GgrsEvent::DesyncDetected { .. })));

    Ok(())
}
