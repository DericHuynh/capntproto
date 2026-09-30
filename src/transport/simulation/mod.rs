//! Private packet IO for exercising the production driver under controlled
//! delivery and readiness. No OS sockets or production entropy overrides.
#![forbid(unsafe_code)]

use futures::future::poll_fn;
use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    io,
    net::SocketAddr,
    rc::Rc,
    task::{Poll, Waker},
};

#[derive(Clone, Debug)]
struct Packet {
    from: SocketAddr,
    to: SocketAddr,
    bytes: Vec<u8>,
}
#[derive(Clone, Copy, Debug)]
enum Send {
    Ready,
    Blocked,
    Fail(io::ErrorKind),
}
struct Endpoint {
    address: SocketAddr,
    open: bool,
    send: Send,
    receive_error: Option<io::ErrorKind>,
    incoming: VecDeque<Packet>,
    send_wait: Vec<Waker>,
    receive_wait: Option<Waker>,
}
#[derive(Default)]
struct State {
    // Indices are never reused. An old handle cannot close a replacement at the
    // same address, but an in-flight UDP packet can reach that new generation.
    endpoints: Vec<Endpoint>,
    routes: BTreeMap<SocketAddr, usize>,
    outgoing: VecDeque<Packet>,
}
#[derive(Clone, Default)]
struct Network(Rc<RefCell<State>>);
impl Network {
    fn bind(&self, address: SocketAddr) -> Socket {
        let mut state = self.0.borrow_mut();
        assert!(
            !state.routes.contains_key(&address),
            "address already bound"
        );
        let id = state.endpoints.len();
        state.endpoints.push(Endpoint {
            address,
            open: true,
            send: Send::Ready,
            receive_error: None,
            incoming: VecDeque::new(),
            send_wait: Vec::new(),
            receive_wait: None,
        });
        state.routes.insert(address, id);
        Socket {
            network: self.clone(),
            id,
        }
    }
    fn close(&self, id: usize) {
        let wake = {
            let mut state = self.0.borrow_mut();
            let endpoint = &mut state.endpoints[id];
            endpoint.open = false;
            endpoint.incoming.clear();
            let address = endpoint.address;
            let mut wake = std::mem::take(&mut endpoint.send_wait);
            wake.extend(endpoint.receive_wait.take());
            if state.routes.get(&address) == Some(&id) {
                state.routes.remove(&address);
            }
            wake
        };
        for waker in wake {
            waker.wake();
        }
    }
    fn sending(&self, id: usize, mode: Send) {
        let wake = {
            let mut state = self.0.borrow_mut();
            let endpoint = &mut state.endpoints[id];
            endpoint.send = mode;
            std::mem::take(&mut endpoint.send_wait)
        };
        for wake in wake {
            wake.wake();
        }
    }
    fn fail_receive(&self, id: usize, error: io::ErrorKind) {
        let wake = {
            let mut state = self.0.borrow_mut();
            let endpoint = &mut state.endpoints[id];
            endpoint.receive_error = Some(error);
            endpoint.receive_wait.take()
        };
        if let Some(wake) = wake {
            wake.wake();
        }
    }
    fn take(&self, newest: bool) -> Option<Packet> {
        let mut state = self.0.borrow_mut();
        if newest {
            state.outgoing.pop_back()
        } else {
            state.outgoing.pop_front()
        }
    }
    fn deliver(&self, packet: Packet) {
        let wake = {
            let mut state = self.0.borrow_mut();
            let Some(&id) = state.routes.get(&packet.to) else {
                return;
            };
            let endpoint = &mut state.endpoints[id];
            assert!(
                endpoint.incoming.len() < 1024,
                "simulation input bound exhausted"
            );
            endpoint.incoming.push_back(packet);
            endpoint.receive_wait.take()
        };
        if let Some(wake) = wake {
            wake.wake();
        }
    }
}

pub(crate) struct Socket {
    network: Network,
    id: usize,
}
impl Socket {
    pub(super) fn check_open(&self) -> io::Result<()> {
        if self.network.0.borrow().endpoints[self.id].open {
            Ok(())
        } else {
            Err(closed())
        }
    }
    pub(super) fn local_addr(&self) -> io::Result<SocketAddr> {
        self.check_open()?;
        Ok(self.network.0.borrow().endpoints[self.id].address)
    }
    pub(super) async fn send_to(&self, bytes: &[u8], to: SocketAddr) -> io::Result<usize> {
        poll_fn(|cx| match self.try_send_to(bytes, to) {
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                let mut state = self.network.0.borrow_mut();
                let waiters = &mut state.endpoints[self.id].send_wait;
                if !waiters.iter().any(|w| w.will_wake(cx.waker())) {
                    assert!(waiters.len() < 1024, "simulation waiter bound exhausted");
                    waiters.push(cx.waker().clone());
                }
                Poll::Pending
            }
            result => Poll::Ready(result),
        })
        .await
    }
    pub(super) fn try_send_to(&self, bytes: &[u8], to: SocketAddr) -> io::Result<usize> {
        let mut state = self.network.0.borrow_mut();
        let endpoint = &mut state.endpoints[self.id];
        if !endpoint.open {
            return Err(closed());
        }
        match endpoint.send {
            Send::Blocked => Err(io::ErrorKind::WouldBlock.into()),
            Send::Fail(kind) => Err(io::Error::new(kind, "simulated send failure")),
            Send::Ready => {
                let from = endpoint.address;
                assert!(
                    state.outgoing.len() < 1024,
                    "simulation output bound exhausted"
                );
                state.outgoing.push_back(Packet {
                    from,
                    to,
                    bytes: bytes.to_vec(),
                });
                Ok(bytes.len())
            }
        }
    }
    pub(super) async fn recv_from(&self, bytes: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        poll_fn(|cx| {
            let mut state = self.network.0.borrow_mut();
            let endpoint = &mut state.endpoints[self.id];
            if !endpoint.open {
                return Poll::Ready(Err(closed()));
            }
            if let Some(kind) = endpoint.receive_error.take() {
                return Poll::Ready(Err(io::Error::new(kind, "simulated receive failure")));
            }
            if let Some(packet) = endpoint.incoming.pop_front() {
                // Match UDP truncation: consume one whole datagram, returning
                // only the bytes that fit in the caller's receive buffer.
                let n = bytes.len().min(packet.bytes.len());
                bytes[..n].copy_from_slice(&packet.bytes[..n]);
                Poll::Ready(Ok((n, packet.from)))
            } else {
                endpoint.receive_wait = Some(cx.waker().clone());
                Poll::Pending
            }
        })
        .await
    }
}
impl Drop for Socket {
    fn drop(&mut self) {
        self.network.close(self.id);
    }
}
fn closed() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, "simulated socket closed")
}

mod tests;
