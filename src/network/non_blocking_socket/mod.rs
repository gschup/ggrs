use crate::network::udp_msg::UdpMessage;
use std::net::SocketAddr;

mod udp_socket;

pub(crate) use udp_socket::UdpNonBlockingSocket;

pub(crate) trait NonBlockingSocket: std::fmt::Debug + Send + Sync {
    fn send_to(&self, msg: &UdpMessage, addr: SocketAddr);
    fn receive_all_messages(&mut self) -> Vec<(SocketAddr, UdpMessage)>;
}
