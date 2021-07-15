use bincode;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use ggrs::PlayerType;

mod stubs;

#[test]
fn test_create_session() {
    assert!(ggrs::start_synctest_session(2, stubs::INPUT_SIZE, 1).is_ok());
}

#[test]
fn test_add_player() {
    let mut sess = ggrs::start_synctest_session(2, stubs::INPUT_SIZE, 1).unwrap();
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
    assert!(sess.add_player(ggrs::PlayerType::Local, 0).is_ok());
    assert!(sess.add_player(ggrs::PlayerType::Local, 1).is_ok());
    assert!(sess.add_player(ggrs::PlayerType::Local, 2).is_err()); // invalid handle
    assert!(sess.add_player(PlayerType::Remote(addr), 0).is_err()); // remote players not supported
}

#[test]
fn test_start_synctest_session() {
    let mut sess = ggrs::start_synctest_session(2, stubs::INPUT_SIZE, 1).unwrap();
    assert!(sess.add_player(PlayerType::Local, 1).is_ok());
    assert!(sess.start_session().is_ok());
}

#[test]
fn test_advance_frame() {
    let handle = 1;
    let check_distance = 6;
    let mut stub = stubs::GameStub::new();
    let mut sess = ggrs::start_synctest_session(2, stubs::INPUT_SIZE, check_distance).unwrap();
    assert!(sess.add_player(PlayerType::Local, handle).is_ok());
    assert!(sess.start_session().is_ok());

    for i in 0..200 {
        let input: u32 = i;
        let serialized_input = bincode::serialize(&input).unwrap();
        let requests = sess.advance_frame(handle, &serialized_input).unwrap();
        stub.handle_requests(requests);
        assert_eq!(stub.gs.frame, i as i32 + 1); // frame should have advanced
    }
}

#[test]
fn test_advance_frames_with_delayed_input() {
    let handle = 1;
    let check_distance = 6;
    let mut stub = stubs::GameStub::new();
    let mut sess = ggrs::start_synctest_session(2, stubs::INPUT_SIZE, check_distance).unwrap();
    assert!(sess.add_player(PlayerType::Local, handle).is_ok());
    assert!(sess.set_frame_delay(2, handle).is_ok());
    assert!(sess.start_session().is_ok());

    for i in 0..200 {
        let input: u32 = i;
        let serialized_input = bincode::serialize(&input).unwrap();
        let requests = sess.advance_frame(handle, &serialized_input).unwrap();
        stub.handle_requests(requests);
        assert_eq!(stub.gs.frame, i as i32 + 1); // frame should have advanced
    }
}
