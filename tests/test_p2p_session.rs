mod debug_socket;
mod stubs;

use ggrs::{
    Config, DesyncDetection, GgrsError, GgrsEvent, PlayerType, SessionBuilder, SessionState,
    UdpNonBlockingSocket,
};
use serial_test::serial;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use stubs::{StubConfig, StubInput};

use crate::debug_socket::DebugSocket;

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

#[test]
fn test_correction_on_prediction_error() -> Result<(), GgrsError> {
    // No sockets actually created at this addr, just using socket type to re-use code from StubConfig.
    let addr1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 7777);
    let addr2 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8888);
    let addr3 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 9999);
    let sockets = DebugSocket::build_sockets(vec![addr1, addr2, addr3]);
    let mut socket1 = sockets.get(0).unwrap().clone();
    let mut socket2 = sockets.get(1).unwrap().clone();
    let mut socket3 = sockets.get(2).unwrap().clone();

    let mut sess1 = SessionBuilder::<StubConfig>::new()
        .with_num_players(3)
        .add_player(PlayerType::Local, 0)?
        .add_player(PlayerType::Remote(addr2), 1)?
        .add_player(PlayerType::Remote(addr3), 2)?
        .with_input_delay(0)
        .with_desync_detection_mode(DesyncDetection::On { interval: 1 })
        .with_max_prediction_window(3)
        .unwrap()
        .start_p2p_session(socket1.clone())?;

    let mut sess2 = SessionBuilder::<StubConfig>::new()
        .with_num_players(3)
        .add_player(PlayerType::Remote(addr1), 0)?
        .add_player(PlayerType::Local, 1)?
        .add_player(PlayerType::Remote(addr3), 2)?
        .with_input_delay(0)
        .with_desync_detection_mode(DesyncDetection::On { interval: 1 })
        .with_max_prediction_window(3)
        .unwrap()
        // .with_input_delay(5)
        .start_p2p_session(socket2.clone())?;

    let mut sess3 = SessionBuilder::<StubConfig>::new()
        .with_num_players(3)
        .add_player(PlayerType::Remote(addr1), 0)?
        .add_player(PlayerType::Remote(addr2), 1)?
        .add_player(PlayerType::Local, 2)?
        .with_input_delay(0)
        .with_desync_detection_mode(DesyncDetection::On { interval: 1 })
        .with_max_prediction_window(3)
        .unwrap()
        // .with_input_delay(5)
        .start_p2p_session(socket3.clone())?;

    while sess1.current_state() != SessionState::Running
        && sess2.current_state() != SessionState::Running
        && sess3.current_state() != SessionState::Running
    {
        sess1.poll_remote_clients();
        sess2.poll_remote_clients();
        sess3.poll_remote_clients();
        socket1.flush_message();
        socket2.flush_message();
        socket3.flush_message();
    }

    // drain events
    assert!(sess1
        .events()
        .chain(sess2.events().chain(sess3.events()))
        .all(|e| matches!(
            e,
            GgrsEvent::Synchronizing { .. } | GgrsEvent::Synchronized { .. }
        )));

    let mut stub1 = stubs::GameStub::new();
    let mut stub2 = stubs::GameStub::new();
    let mut stub3 = stubs::GameStub::new();

    // Advance all clients with no input + flush messages
    {
        sess1.poll_remote_clients();
        sess2.poll_remote_clients();
        sess3.poll_remote_clients();

        sess1.add_local_input(0, StubInput { inp: 0 }).unwrap();
        sess2.add_local_input(1, StubInput { inp: 0 }).unwrap();
        sess3.add_local_input(2, StubInput { inp: 0 }).unwrap();

        let requests1 = sess1.advance_frame().unwrap();
        let requests2 = sess2.advance_frame().unwrap();
        let requests3 = sess3.advance_frame().unwrap();

        stub1.handle_requests(requests1);
        stub2.handle_requests(requests2);
        stub3.handle_requests(requests3);

        socket1.flush_message();
        socket2.flush_message();
        socket3.flush_message();
    }

    // client 1 changes input to 1
    // - No messages flushed this step so 2/3 predict 1's old input
    {
        sess1.poll_remote_clients();
        sess2.poll_remote_clients();
        sess3.poll_remote_clients();

        sess1.add_local_input(0, StubInput { inp: 1 }).unwrap();
        sess2.add_local_input(1, StubInput { inp: 0 }).unwrap();
        sess3.add_local_input(2, StubInput { inp: 0 }).unwrap();

        let requests1 = sess1.advance_frame().unwrap();
        let requests2 = sess2.advance_frame().unwrap();
        let requests3 = sess3.advance_frame().unwrap();

        stub1.handle_requests(requests1);
        stub2.handle_requests(requests2);
        stub3.handle_requests(requests3);

        // socket1.flush_message();
        // socket2.flush_message();
        // socket3.flush_message();
    }

    // Same inputs, deliver messages sent from 1 and 3, but not 2.
    // - Next frame 3 will see it needs to rollback due to change in 1's input.
    // - 3 will hit PredictionThreshold error, due to not having 2's inputs for last couple frames.
    {
        sess1.poll_remote_clients();
        sess2.poll_remote_clients();
        sess3.poll_remote_clients();

        sess1.add_local_input(0, StubInput { inp: 1 }).unwrap();
        sess2.add_local_input(1, StubInput { inp: 0 }).unwrap();
        sess3.add_local_input(2, StubInput { inp: 0 }).unwrap();

        let requests1 = sess1.advance_frame().unwrap();
        let requests2 = sess2.advance_frame().unwrap();
        let requests3 = sess3.advance_frame().unwrap();

        stub1.handle_requests(requests1);
        stub2.handle_requests(requests2);
        stub3.handle_requests(requests3);

        socket1.flush_message();
        // socket2.flush_message();
        socket3.flush_message();
    }

    // 3 should determine it must rollback due to receiving change in 1's input.
    // - 3 hits prediction threshold due to missing 2's inputs.
    {
        sess1.poll_remote_clients();
        sess2.poll_remote_clients();
        sess3.poll_remote_clients();

        sess1.add_local_input(0, StubInput { inp: 1 }).unwrap();
        sess2.add_local_input(1, StubInput { inp: 0 }).unwrap();
        sess3.add_local_input(2, StubInput { inp: 0 }).unwrap();

        let requests2 = sess2.advance_frame().unwrap();
        stub2.handle_requests(requests2);

        //
        let result1 = sess1.advance_frame();
        assert!(matches!(result1, Err(GgrsError::PredictionThreshold)));
        let result3 = sess3.advance_frame();
        assert!(matches!(result3, Err(GgrsError::PredictionThreshold)));

        socket1.flush_message();
        socket2.flush_message();
        socket3.flush_message();
    }

    // In failure case, 3 should've rolled back last frame, but due to PredictionThreshold,
    // the rollback requests were not delivered. If not handled correctly, this may cause desync.

    // Perform couple more steps to let clients catch up on desync detection so we may validate
    // - No change in inputs, all messages delivered
    for _ in 0..2 {
        sess1.poll_remote_clients();
        sess2.poll_remote_clients();
        sess3.poll_remote_clients();

        sess1.add_local_input(0, StubInput { inp: 1 }).unwrap();
        sess2.add_local_input(1, StubInput { inp: 0 }).unwrap();
        sess3.add_local_input(2, StubInput { inp: 0 }).unwrap();

        let requests1 = sess1.advance_frame().unwrap();
        let requests2 = sess2.advance_frame().unwrap();
        let requests3 = sess3.advance_frame().unwrap();

        stub1.handle_requests(requests1);
        stub2.handle_requests(requests2);
        stub3.handle_requests(requests3);

        socket1.flush_message();
        socket2.flush_message();
        socket3.flush_message();
    }

    // Verify all clients are in sync, even after 3 hit PredictionThreshold on same frame it had to rollback.
    assert!(sess1
        .events()
        .chain(sess2.events().chain(sess3.events()))
        .all(|e| !matches!(e, GgrsEvent::DesyncDetected { .. })));

    Ok(())
}
