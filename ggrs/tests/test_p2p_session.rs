use ggrs::GGRSSession;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use serial_test::serial;

mod stubs;

#[test]
#[serial]
fn test_create_session() {
    assert!(ggrs::start_p2p_session(2, std::mem::size_of::<u32>(), 7777).is_ok());
}

#[test]
#[serial]
fn test_add_player() {
    let mut sess = ggrs::start_p2p_session(2, std::mem::size_of::<u32>(), 7777).unwrap();
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
    assert!(sess.add_player(ggrs::PlayerType::Local, 0).is_ok());
    assert!(sess.add_player(ggrs::PlayerType::Remote(addr), 1).is_ok());
    assert!(sess.add_player(ggrs::PlayerType::Remote(addr), 1).is_err()); // handle already registered
    assert!(sess.add_player(ggrs::PlayerType::Remote(addr), 2).is_err()); // invalid handle
}

#[test]
#[serial]
fn test_start_session() {
    let mut sess = ggrs::start_p2p_session(2, std::mem::size_of::<u32>(), 7777).unwrap();
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
    assert!(sess.add_player(ggrs::PlayerType::Local, 0).is_ok());
    assert!(sess.start_session().is_err()); // not enough players
    assert!(sess.add_player(ggrs::PlayerType::Remote(addr), 1).is_ok());
    assert!(sess.start_session().is_ok()); // works
    assert!(sess.start_session().is_err()); // cannot start twice
}

#[test]
#[serial]
fn test_disconnect_player() {
    let mut sess = ggrs::start_p2p_session(2, std::mem::size_of::<u32>(), 7777).unwrap();
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
    assert!(sess.add_player(ggrs::PlayerType::Local, 0).is_ok());
    assert!(sess.add_player(ggrs::PlayerType::Remote(addr), 1).is_ok());
    assert!(sess.start_session().is_ok());
    assert!(sess.disconnect_player(0).is_err()); // for now, local players cannot be disconnected
    assert!(sess.disconnect_player(1).is_ok());
    assert!(sess.disconnect_player(1).is_err()); // already disconnected
}
