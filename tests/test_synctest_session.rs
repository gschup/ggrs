mod stubs;

use ggrs::{GGRSError, SyncTestSessionBuilder};
use stubs::{StubConfig, StubInput};

#[test]
fn test_create_session() {
    assert!(SyncTestSessionBuilder::<StubConfig>::new(2)
        .start_session()
        .is_ok());
}

#[test]
fn test_advance_frame_with_rollbacks() -> Result<(), GGRSError> {
    let check_distance = 7;
    let mut stub = stubs::GameStub::new();
    let mut sess = SyncTestSessionBuilder::new(2)
        .with_check_distance(check_distance)
        .start_session()?;

    for i in 0..200 {
        let inputs = vec![StubInput { inp: i }, StubInput { inp: i }];
        let requests = sess.advance_frame(&inputs).unwrap();
        stub.handle_requests(requests);
        assert_eq!(stub.gs.frame, i as i32 + 1); // frame should have advanced
    }

    Ok(())
}

#[test]
fn test_advance_frames_with_delayed_input() -> Result<(), GGRSError> {
    let check_distance = 7;
    let mut stub = stubs::GameStub::new();
    let mut sess = SyncTestSessionBuilder::new(2)
        .with_check_distance(check_distance)
        .with_input_delay(2)
        .start_session()?;

    for i in 0..200 {
        let inputs = vec![StubInput { inp: i }, StubInput { inp: i }];
        let requests = sess.advance_frame(&inputs).unwrap();
        stub.handle_requests(requests);
        assert_eq!(stub.gs.frame, i as i32 + 1); // frame should have advanced
    }

    Ok(())
}

#[test]
#[should_panic]
fn test_advance_frames_with_random_checksums() {
    let mut stub = stubs::RandomChecksumGameStub::new();
    let mut sess = SyncTestSessionBuilder::new(2)
        .with_input_delay(2)
        .start_session()
        .unwrap();

    for i in 0..200 {
        let inputs = vec![StubInput { inp: i }, StubInput { inp: i }];
        let requests = sess.advance_frame(&inputs).unwrap();
        stub.handle_requests(requests);
        assert_eq!(stub.gs.frame, i as i32 + 1); // frame should have advanced
    }
}
