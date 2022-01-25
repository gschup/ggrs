mod stubs;

use ggrs::SyncTestSession;
use stubs::{StubConfig, StubInput};

#[test]
fn test_create_session() {
    assert!(SyncTestSession::<StubConfig>::new(2, stubs::MAX_PRED_FRAMES, 2).is_ok());
}

#[test]
fn test_advance_frame_with_rollbacks() {
    let check_distance = 7;
    let mut stub = stubs::GameStub::new();
    let mut sess = SyncTestSession::new(2, stubs::MAX_PRED_FRAMES, check_distance).unwrap();

    for i in 0..200 {
        let inputs = vec![StubInput { inp: i }, StubInput { inp: i }];
        let requests = sess.advance_frame(&inputs).unwrap();
        stub.handle_requests(requests);
        assert_eq!(stub.gs.frame, i as i32 + 1); // frame should have advanced
    }
}

#[test]
fn test_advance_frames_with_delayed_input() {
    let handle = 1;
    let check_distance = 7;
    let mut stub = stubs::GameStub::new();
    let mut sess = SyncTestSession::new(2, stubs::MAX_PRED_FRAMES, check_distance).unwrap();
    assert!(sess.set_frame_delay(2, handle).is_ok());

    for i in 0..200 {
        let inputs = vec![StubInput { inp: i }, StubInput { inp: i }];
        let requests = sess.advance_frame(&inputs).unwrap();
        stub.handle_requests(requests);
        assert_eq!(stub.gs.frame, i as i32 + 1); // frame should have advanced
    }
}

#[test]
#[should_panic]
fn test_advance_frames_with_random_checksums() {
    let handle = 1;
    let check_distance = 2;
    let mut stub = stubs::RandomChecksumGameStub::new();
    let mut sess = SyncTestSession::new(2, stubs::MAX_PRED_FRAMES, check_distance).unwrap();
    assert!(sess.set_frame_delay(2, handle).is_ok());

    for i in 0..200 {
        let inputs = vec![StubInput { inp: i }, StubInput { inp: i }];
        let requests = sess.advance_frame(&inputs).unwrap();
        stub.handle_requests(requests);
        assert_eq!(stub.gs.frame, i as i32 + 1); // frame should have advanced
    }
}
