use bincode;
use ggrs::SyncTestSession;

mod stubs;

#[test]
fn test_create_session() {
    assert!(SyncTestSession::new(2, stubs::INPUT_SIZE, stubs::MAX_PRED_FRAMES, 2).is_ok());
}

#[test]
fn test_advance_frame_with_rollbacks() {
    let check_distance = 7;
    let mut stub = stubs::GameStub::new();
    let mut sess =
        SyncTestSession::new(2, stubs::INPUT_SIZE, stubs::MAX_PRED_FRAMES, check_distance).unwrap();

    for i in 0..200 {
        let input: u32 = i;
        let mut serialized_input = Vec::new();
        serialized_input.push(bincode::serialize(&input).unwrap());
        serialized_input.push(bincode::serialize(&input).unwrap());
        let requests = sess.advance_frame(&serialized_input).unwrap();
        stub.handle_requests(requests);
        assert_eq!(stub.gs.frame, i as i32 + 1); // frame should have advanced
    }
}

#[test]
fn test_advance_frames_with_delayed_input() {
    let handle = 1;
    let check_distance = 7;
    let mut stub = stubs::GameStub::new();
    let mut sess =
        SyncTestSession::new(2, stubs::INPUT_SIZE, stubs::MAX_PRED_FRAMES, check_distance).unwrap();
    assert!(sess.set_frame_delay(2, handle).is_ok());

    for i in 0..200 {
        let input: u32 = i;
        let mut serialized_input = Vec::new();
        serialized_input.push(bincode::serialize(&input).unwrap());
        serialized_input.push(bincode::serialize(&input).unwrap());
        let requests = sess.advance_frame(&serialized_input).unwrap();
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
    let mut sess =
        SyncTestSession::new(2, stubs::INPUT_SIZE, stubs::MAX_PRED_FRAMES, check_distance).unwrap();
    assert!(sess.set_frame_delay(2, handle).is_ok());

    for i in 0..200 {
        let input: u32 = i;
        let mut serialized_input = Vec::new();
        serialized_input.push(bincode::serialize(&input).unwrap());
        serialized_input.push(bincode::serialize(&input).unwrap());
        let requests = sess.advance_frame(&serialized_input).unwrap();
        stub.handle_requests(requests);
        assert_eq!(stub.gs.frame, i as i32 + 1); // frame should have advanced
    }
}
