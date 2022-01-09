use std::{
    io::ErrorKind,
    net::{SocketAddr, ToSocketAddrs, UdpSocket},
};

use crate::network::udp_msg::UdpMessage;

use super::NonBlockingSocket;

const RECV_BUFFER_SIZE: usize = 4096;

#[derive(Debug)]
pub(crate) struct UdpNonBlockingSocket {
    socket: UdpSocket,
    buffer: [u8; RECV_BUFFER_SIZE],
}

impl UdpNonBlockingSocket {
    pub(crate) fn new<A: ToSocketAddrs>(addr: A) -> Result<Self, std::io::Error> {
        let socket = UdpSocket::bind(addr)?;
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket,
            buffer: [0; RECV_BUFFER_SIZE],
        })
    }
}

impl NonBlockingSocket for UdpNonBlockingSocket {
    fn send_to(&mut self, msg: &UdpMessage, addr: SocketAddr) {
        let buf = bincode::serialize(&msg).unwrap();
        self.socket.send_to(&buf, addr).unwrap();
    }

    fn receive_all_messages(&mut self) -> Vec<(SocketAddr, UdpMessage)> {
        let mut received_messages = Vec::new();
        loop {
            match self.socket.recv_from(&mut self.buffer) {
                Ok((number_of_bytes, src_addr)) => {
                    assert!(number_of_bytes <= RECV_BUFFER_SIZE);
                    if let Ok(msg) = bincode::deserialize(&self.buffer[0..number_of_bytes]) {
                        received_messages.push((src_addr, msg));
                    }
                }
                // there are no more messages
                Err(ref err) if err.kind() == ErrorKind::WouldBlock => return received_messages,
                // datagram socket sometimes get this error as a result of calling the send_to method
                Err(ref err) if err.kind() == ErrorKind::ConnectionReset => continue,
                // all other errors cause a panic
                Err(err) => panic!("{:?}: {} on {:?}", err.kind(), err, &self.socket),
            }
        }
    }
}
