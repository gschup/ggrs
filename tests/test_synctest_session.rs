mod stubs;
mod stubs_enum;

use ggrs::{GgrsError, GGRSRequest, SessionBuilder};
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
            assert!(matches!(requests[0], GGRSRequest::SaveGameState { .. }));
            assert!(matches!(requests[1], GGRSRequest::AdvanceFrame { .. }));
        } else {
            assert_eq!(requests.len(), 6); // load, advance, save, advance, save, advance
            assert!(matches!(requests[0], GGRSRequest::LoadGameState { .. })); // rollback
            assert!(matches!(requests[1], GGRSRequest::AdvanceFrame { .. })); // rollback
            assert!(matches!(requests[2], GGRSRequest::SaveGameState { .. })); // rollback
            assert!(matches!(requests[3], GGRSRequest::AdvanceFrame { .. })); // rollback
            assert!(matches!(requests[4], GGRSRequest::SaveGameState { .. }));
            assert!(matches!(requests[5], GGRSRequest::AdvanceFrame { .. }));
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
