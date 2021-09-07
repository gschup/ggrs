use crate::network::udp_msg::UdpMessage;
use std::net::SocketAddr;

mod udp_socket;

pub(crate) use udp_socket::UdpNonBlockingSocket;

/// This `NonBlockingSocket` trait is used when you want to use GGRS with your own socket.
/// However you wish to send and receive messages, it should be implemented through these two methods.
/// Messages should be sent in an UDP-like fashion, unordered and unreliable.
/// GGRS has an internal protocol on top of this to make sure all important information is sent and received.
pub trait NonBlockingSocket: std::fmt::Debug + Send + Sync {
    /// Takes an `UdpMessage` and sends it to the given `SocketAddr`.
    fn send_to(&mut self, msg: &UdpMessage, addr: SocketAddr);

    /// This method should return all messages received since the last time this method was called. `
    /// The pairs `(SocketAddr, UdpMessage)` indicate from which adress each packet was received.
    fn receive_all_messages(&mut self) -> Vec<(SocketAddr, UdpMessage)>;
}
