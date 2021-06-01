use crate::network::udp_msg::UdpMessage;
use std::io::ErrorKind;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};

const RECV_BUFFER_SIZE: usize = 4096;
pub struct NonBlockingSocket {
    socket: UdpSocket,
    buffer: [u8; RECV_BUFFER_SIZE],
}

impl NonBlockingSocket {
    pub(crate) fn new<A: ToSocketAddrs>(addr: A) -> Self {
        let socket = UdpSocket::bind(addr).unwrap();
        socket.set_nonblocking(true).unwrap();
        Self {
            socket,
            buffer: [0; RECV_BUFFER_SIZE],
        }
    }

    pub(crate) fn send_to<A: ToSocketAddrs>(&self, msg: UdpMessage, addr: A) {
        let buf = bincode::serialize(&msg).unwrap();
        self.socket.send_to(&buf, addr).unwrap();
    }

    pub(crate) fn receive_all_messages(&mut self) -> Vec<(SocketAddr, UdpMessage)> {
        let mut received_messages = Vec::new();
        loop {
            match self.socket.recv_from(&mut self.buffer) {
                Ok((number_of_bytes, src_addr)) => {
                    assert!(number_of_bytes <= RECV_BUFFER_SIZE);
                    let msg = bincode::deserialize(&self.buffer[0..number_of_bytes]).unwrap();
                    received_messages.push((src_addr, msg));
                }
                Err(ref err) if err.kind() == ErrorKind::WouldBlock => return received_messages,
                Err(err) => panic!("UDP error: {}", err),
            }
        }
    }
}
