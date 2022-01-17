use crate::network::udp_msg::UdpMessage;

mod udp_socket;

pub(crate) use udp_socket::UdpNonBlockingSocket;

/// This `NonBlockingSocket` trait is used when you want to use GGRS with your own socket.
/// However you wish to send and receive messages, it should be implemented through these two methods.
/// Messages should be sent in an UDP-like fashion, unordered and unreliable.
/// GGRS has an internal protocol on top of this to make sure all important information is sent and received.
/// Disable feature `send_socket` to remove the Send + Sync bounds. Note: bevy_ggrs requires `send_socket` to be enabled.
#[cfg(feature = "send_socket")]
pub trait NonBlockingSocket<A>: Send + Sync {
    /// Takes an `UdpMessage` and sends it to the given address.
    fn send_to(&mut self, msg: &UdpMessage, addr: &A);

    /// This method should return all messages received since the last time this method was called. `
    /// The pairs `(A, UdpMessage)` indicate from which address each packet was received.
    fn receive_all_messages(&mut self) -> Vec<(A, UdpMessage)>;
}
#[cfg(not(feature = "send_socket"))]
pub trait NonBlockingSocket<A> {
    fn send_to(&mut self, msg: &UdpMessage, addr: &A);
    fn receive_all_messages(&mut self) -> Vec<(A, UdpMessage)>;
}
