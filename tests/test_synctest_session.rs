mod stubs;
mod stubs_enum;

use ggrs::{GgrsError, GgrsRequest, SessionBuilder};
use stubs::{StubConfig, StubInput};

#[test]
fn test_create_session() {
    assert!(SessionBuilder::<StubConfig>::new()
        .start_synctest_session()
        .is_ok());
}

#[test]
fn test_advance_frame_no_rollbacks() -> Result<(), GgrsError> {
    let check_distance = 0;
    let mut stub = stubs::GameStub::new();
    let mut sess = SessionBuilder::new()
        .with_check_distance(check_distance)
        .start_synctest_session()?;

    for i in 0..200 {
        sess.add_local_input(0, StubInput { inp: i })?;
        sess.add_local_input(1, StubInput { inp: i })?;
        let requests = sess.advance_frame()?;
        assert_eq!(requests.len(), 1); // only advance
        stub.handle_requests(requests);
        assert_eq!(stub.gs.frame, i as i32 + 1); // frame should have advanced
    }

    Ok(())
}

#[test]
fn test_advance_frame_with_rollbacks() -> Result<(), GgrsError> {
    let check_distance = 2;
    let mut stub = stubs::GameStub::new();
    let mut sess = SessionBuilder::new()
        .with_check_distance(check_distance)
        .start_synctest_session()?;

    for i in 0..200 {
        sess.add_local_input(0, StubInput { inp: i as u32 })?;
        sess.add_local_input(1, StubInput { inp: i as u32 })?;
        let requests = sess.advance_frame()?;
        if i <= check_distance {
            assert_eq!(requests.len(), 2); // save, advance
            assert!(matches!(requests[0], GgrsRequest::SaveGameState { .. }));
            assert!(matches!(requests[1], GgrsRequest::AdvanceFrame { .. }));
        } else {
            assert_eq!(requests.len(), 6); // load, advance, save, advance, save, advance
            assert!(matches!(requests[0], GgrsRequest::LoadGameState { .. })); // rollback
            assert!(matches!(requests[1], GgrsRequest::AdvanceFrame { .. })); // rollback
            assert!(matches!(requests[2], GgrsRequest::SaveGameState { .. })); // rollback
            assert!(matches!(requests[3], GgrsRequest::AdvanceFrame { .. })); // rollback
            assert!(matches!(requests[4], GgrsRequest::SaveGameState { .. }));
            assert!(matches!(requests[5], GgrsRequest::AdvanceFrame { .. }));
        }

        stub.handle_requests(requests);
        assert_eq!(stub.gs.frame, i as i32 + 1); // frame should have advanced
    }

    Ok(())
}

#[test]
fn test_advance_frames_with_delayed_input() -> Result<(), GgrsError> {
    let check_distance = 7;
    let mut stub = stubs::GameStub::new();
    let mut sess = SessionBuilder::new()
        .with_check_distance(check_distance)
        .with_input_delay(2)
        .start_synctest_session()?;

    for i in 0..200 {
        sess.add_local_input(0, StubInput { inp: i })?;
        sess.add_local_input(1, StubInput { inp: i })?;
        let requests = sess.advance_frame()?;
        stub.handle_requests(requests);
        assert_eq!(stub.gs.frame, i as i32 + 1); // frame should have advanced
    }

    Ok(())
}

#[test]
fn test_builder_sparse_saving_errors_for_synctest() {
    let result = SessionBuilder::<StubConfig>::new()
        .with_sparse_saving_mode(true)
        .start_synctest_session();
    assert!(matches!(result, Err(GgrsError::InvalidRequest { .. })));
}

#[test]
fn test_create_session_single_player() -> Result<(), GgrsError> {
    let mut stub = stubs::GameStub1P::new();
    let mut sess = SessionBuilder::<StubConfig>::new()
        .with_num_players(1)?
        .start_synctest_session()?;

    for i in 0..20 {
        sess.add_local_input(0, StubInput { inp: i })?;
        let requests = sess.advance_frame()?;
        stub.handle_requests(requests);
        assert_eq!(stub.gs.frame, i as i32 + 1);
    }

    Ok(())
}

#[test]
fn test_advance_frame_check_distance_one() -> Result<(), GgrsError> {
    let check_distance = 1;
    let mut stub = stubs::GameStub::new();
    let mut sess = SessionBuilder::new()
        .with_check_distance(check_distance)
        .start_synctest_session()?;

    for i in 0..20 {
        sess.add_local_input(0, StubInput { inp: i as u32 })?;
        sess.add_local_input(1, StubInput { inp: i as u32 })?;
        let requests = sess.advance_frame()?;

        if i <= check_distance {
            // not yet far enough to trigger a rollback
            assert_eq!(requests.len(), 2, "frame {i}: expected [Save, Advance]");
            assert!(matches!(requests[0], GgrsRequest::SaveGameState { .. }));
            assert!(matches!(requests[1], GgrsRequest::AdvanceFrame { .. }));
        } else {
            // rollback 1 frame + re-advance + save + advance
            assert_eq!(
                requests.len(),
                4,
                "frame {i}: expected [Load, Advance, Save, Advance]"
            );
            assert!(matches!(requests[0], GgrsRequest::LoadGameState { .. }));
            assert!(matches!(requests[1], GgrsRequest::AdvanceFrame { .. }));
            assert!(matches!(requests[2], GgrsRequest::SaveGameState { .. }));
            assert!(matches!(requests[3], GgrsRequest::AdvanceFrame { .. }));
        }

        stub.handle_requests(requests);
        assert_eq!(stub.gs.frame, i as i32 + 1);
    }

    Ok(())
}

#[test]
fn test_state_is_deterministic() -> Result<(), GgrsError> {
    // Two independent sessions with the same inputs must reach the same final state,
    // verifying that save/load/advance is fully deterministic.
    let mut stub_a = stubs::GameStub::new();
    let mut sess_a = SessionBuilder::new()
        .with_check_distance(2)
        .start_synctest_session()?;

    let mut stub_b = stubs::GameStub::new();
    let mut sess_b = SessionBuilder::new()
        .with_check_distance(2)
        .start_synctest_session()?;

    for i in 0..100u32 {
        let inp = StubInput { inp: i % 7 }; // non-trivial pattern

        sess_a.add_local_input(0, inp)?;
        sess_a.add_local_input(1, inp)?;
        stub_a.handle_requests(sess_a.advance_frame()?);

        sess_b.add_local_input(0, inp)?;
        sess_b.add_local_input(1, inp)?;
        stub_b.handle_requests(sess_b.advance_frame()?);
    }

    assert_eq!(stub_a.gs.frame, stub_b.gs.frame);
    assert_eq!(stub_a.gs.state, stub_b.gs.state);

    Ok(())
}

#[test]
#[should_panic]
fn test_advance_frames_with_random_checksums() {
    let mut stub = stubs::RandomChecksumGameStub::new();
    let mut sess = SessionBuilder::new()
        .with_input_delay(2)
        .start_synctest_session()
        .unwrap();

    for i in 0..200 {
        sess.add_local_input(0, StubInput { inp: i }).unwrap();
        sess.add_local_input(1, StubInput { inp: i }).unwrap();
        let requests = sess.advance_frame().unwrap(); // this should give a MismatchedChecksum error
        stub.handle_requests(requests);
        assert_eq!(stub.gs.frame, i as i32 + 1);
    }
}
