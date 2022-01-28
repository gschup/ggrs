mod stubs;

use ggrs::{GGRSError, SessionBuilder};
use stubs::{StubConfig, StubInput};

#[test]
fn test_create_session() {
    assert!(SessionBuilder::<StubConfig>::new()
        .start_synctest_session()
        .is_ok());
}

#[test]
fn test_advance_frame_with_rollbacks() -> Result<(), GGRSError> {
    let check_distance = 7;
    let mut stub = stubs::GameStub::new();
    let mut sess = SessionBuilder::new()
        .with_check_distance(check_distance)
        .start_synctest_session()?;

    for i in 0..200 {
        sess.add_local_input(0, StubInput { inp: i }).unwrap();
        sess.add_local_input(1, StubInput { inp: i }).unwrap();
        let requests = sess.advance_frame().unwrap();
        stub.handle_requests(requests);
        assert_eq!(stub.gs.frame, i as i32 + 1); // frame should have advanced
    }

    Ok(())
}

#[test]
fn test_advance_frames_with_delayed_input() -> Result<(), GGRSError> {
    let check_distance = 7;
    let mut stub = stubs::GameStub::new();
    let mut sess = SessionBuilder::new()
        .with_check_distance(check_distance)
        .with_input_delay(2)
        .start_synctest_session()?;

    for i in 0..200 {
        sess.add_local_input(0, StubInput { inp: i }).unwrap();
        sess.add_local_input(1, StubInput { inp: i }).unwrap();
        let requests = sess.advance_frame().unwrap();
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
        let requests = sess.advance_frame().unwrap();
        stub.handle_requests(requests);
        assert_eq!(stub.gs.frame, i as i32 + 1); // frame should have advanced
    }
}
