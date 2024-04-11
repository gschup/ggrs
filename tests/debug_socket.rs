use std::{
    collections::HashMap,
    hash::Hash,
    sync::{Arc, Mutex},
};

pub type MessageBuffer<A> = Vec<(A, ggrs::Message)>;

/// A dummy socket for reproducing controlled delays in delivering messages.
///
/// No messages sent will be made available to receiver unless test implementor
/// explicitly flushes message. This allows implementing tests that wish to reproduce
/// precise delays in message delivery.
///
/// [`DebugSocket::build_sockets`] will generate connected sockets for all addresses.
#[derive(Default, Clone)]
pub struct DebugSocket<A: Clone + PartialEq + Eq + Hash> {
    /// Messages sent, but not yet flushed to be made available to receiver.
    sent_messages: HashMap<A, Arc<Mutex<Vec<ggrs::Message>>>>,

    /// Message buffers per address that are shared between all sockets.
    /// When socket flushes messages to recepient, messages moved from sent buffer
    /// to remote buffer.
    remote_delivery_buffers: HashMap<A, Arc<Mutex<MessageBuffer<A>>>>,

    /// Delivered messages ready for consumption for local owner of socket
    received_messages: Arc<Mutex<MessageBuffer<A>>>,

    /// Address of local socket
    local_addr: A,
}

impl<A> DebugSocket<A>
where
    A: Clone + PartialEq + Eq + Hash,
{
    /// Build socket for each address such that each one can write to
    /// any other address.
    pub fn build_sockets(addrs: Vec<A>) -> Vec<DebugSocket<A>> {
        // Create shared buffer for each address
        let receive_buffers: HashMap<A, Arc<Mutex<MessageBuffer<A>>>> =
            addrs.iter().fold(Default::default(), |mut map, addr| {
                map.insert(addr.clone(), Arc::new(Mutex::new(vec![])));
                map
            });

        let mut sockets = Vec::<DebugSocket<A>>::default();
        for addr in addrs.clone() {
            sockets.push(DebugSocket {
                sent_messages: addrs.iter().fold(Default::default(), |mut map, addr| {
                    map.insert(addr.clone(), Arc::new(Mutex::new(vec![])));
                    map
                }),
                remote_delivery_buffers: receive_buffers.clone(),
                // Receive message from delivery buffer for this address
                received_messages: receive_buffers.get(&addr).unwrap().clone(),
                local_addr: addr,
            })
        }
        sockets
    }

    /// Deliver messages sent to other receiving sockets
    pub fn flush_message(&mut self) {
        for (addr, sent) in self.sent_messages.iter_mut() {
            let mut sent = sent.lock().unwrap();
            let mut remote_buffer = self
                .remote_delivery_buffers
                .get_mut(addr)
                .unwrap()
                .lock()
                .unwrap();

            remote_buffer.extend(sent.drain(..).map(|m| (self.local_addr.clone(), m)));
        }
    }

    /// Deliver messages sent to specified address
    #[allow(dead_code)]
    pub fn flush_message_for_addr(&mut self, addr: A) {
        let mut sent = self.sent_messages.get_mut(&addr).unwrap().lock().unwrap();
        let mut remote_buffer = self
            .remote_delivery_buffers
            .get_mut(&addr)
            .unwrap()
            .lock()
            .unwrap();

        remote_buffer.extend(sent.drain(..).map(|m| (self.local_addr.clone(), m)));
    }
}

impl<A: Clone + PartialEq + Eq + Hash> ggrs::NonBlockingSocket<A> for DebugSocket<A> {
    fn send_to(&mut self, msg: &ggrs::Message, addr: &A) {
        let mut sent = self.sent_messages.get_mut(addr).unwrap().lock().unwrap();
        sent.push(msg.clone());
    }

    fn receive_all_messages(&mut self) -> Vec<(A, ggrs::Message)> {
        let mut messages = self.received_messages.lock().unwrap();
        messages.drain(..).collect()
    }
}
