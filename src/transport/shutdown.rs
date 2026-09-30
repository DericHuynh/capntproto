use crate::noise_shutdown::{Control, Frame, Protocol, FRAME_BYTES};
use std::{collections::VecDeque, io};

pub(super) const ACKNOWLEDGED_CLOSE: u64 = 0x525053;

pub(super) struct ShutdownDriver {
    pub control: Control,
    protocol: Protocol,
    nonce: [u8; 16],
    outgoing_id: u64,
    incoming_id: u64,
    outgoing: VecDeque<(u64, [u8; FRAME_BYTES])>,
    send_offset: usize,
    incoming: [[u8; FRAME_BYTES + 1]; 2],
    receive_offset: [usize; 2],
    received: [bool; 2],
    ack_written: bool,
    ack_delivered: bool,
}
impl ShutdownDriver {
    pub fn new(control: Control, server: bool) -> Self {
        Self {
            control,
            protocol: Protocol::default(),
            nonce: super::cid(),
            outgoing_id: if server { 3 } else { 2 },
            incoming_id: if server { 2 } else { 3 },
            outgoing: VecDeque::new(),
            send_offset: 0,
            incoming: [[0; FRAME_BYTES + 1]; 2],
            receive_offset: [0; 2],
            received: [false; 2],
            ack_written: false,
            ack_delivered: false,
        }
    }
    pub fn requested(&self) -> bool {
        self.control.requested()
    }
    pub fn closing(&self) -> bool {
        self.requested() || self.protocol.peer.is_some()
    }
    pub fn delivered(&mut self, bytes: usize) -> io::Result<()> {
        self.protocol.deliver(bytes)
    }
    pub fn acknowledged(&self) -> bool {
        self.protocol.ready(self.ack_delivered)
    }
    pub fn finish_on_close(&self, conn: &quiche::Connection) {
        // A graceful peer close can overtake the transport ACK of our reply.
        // It cannot replace the receipt, reciprocal request, input delivery or
        // complete, unreset reply write. An unrelated close/idle timeout proves
        // none of these. The authenticated application code states the peer completed
        // its side of this exchange; the reason text is diagnostic only.
        if (conn.is_closed() || conn.is_draining())
            && !conn.is_timed_out()
            && conn
                .peer_error()
                .is_some_and(|error| error.is_app && error.error_code == ACKNOWLEDGED_CLOSE)
            && self.protocol.ready(self.ack_written)
            && (!self.protocol.expects_peer
                || conn.stream_send_acknowledged(self.outgoing_id + 4).is_ok())
        {
            self.control.finish(Ok(crate::noise_shutdown::Receipt {
                bytes: self.protocol.sent.unwrap().bytes,
            }));
        }
    }
    pub fn step(
        &mut self,
        conn: &mut quiche::Connection,
        written: u64,
        drained: bool,
    ) -> io::Result<()> {
        if self.requested() && drained && self.protocol.sent.is_none() {
            self.outgoing.push_back((
                self.outgoing_id,
                self.protocol.request(self.nonce, written)?.encode(),
            ));
        }
        // Each direction has one request stream (2/3) and one acknowledgement
        // stream (6/7). Both contain exactly one bounded frame and a FIN.
        for index in 0..2 {
            if self.received[index] {
                continue;
            }
            loop {
                match conn.stream_recv(
                    self.incoming_id + 4 * index as u64,
                    &mut self.incoming[index][self.receive_offset[index]..],
                ) {
                    Ok((n, fin)) => {
                        self.receive_offset[index] += n;
                        if self.receive_offset[index] > FRAME_BYTES
                            || (fin && self.receive_offset[index] != FRAME_BYTES)
                        {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "invalid Noise shutdown control stream",
                            ));
                        }
                        if fin {
                            let frame = Frame::decode(
                                self.incoming[index][..FRAME_BYTES].try_into().unwrap(),
                            )?;
                            if (frame.kind == 1) != (index == 0) {
                                return Err(io::Error::new(
                                    io::ErrorKind::InvalidData,
                                    "wrong Noise shutdown stream",
                                ));
                            }
                            self.protocol.receive(frame)?;
                            self.received[index] = true;
                            if index == 0 {
                                self.control.peer_closing();
                            }
                            break;
                        }
                        if n == 0 {
                            break;
                        }
                    }
                    Err(quiche::Error::Done) | Err(quiche::Error::InvalidStreamState(_)) => break,
                    Err(e) => return Err(super::error(e)),
                }
            }
        }
        // A crossed shutdown advertises its own request in the ACK. Wait for
        // the local writer fence before emitting that ACK, even if the peer's
        // request/data arrived first. The remote side then waits for both legs.
        if !self.requested() || self.protocol.sent.is_some() {
            if let Some(ack) = self.protocol.acknowledge() {
                self.outgoing
                    .push_back((self.outgoing_id + 4, ack.encode()));
            }
        }
        while let Some((id, frame)) = self.outgoing.front() {
            match conn.stream_send(*id, &frame[self.send_offset..], true) {
                Ok(n) => {
                    self.send_offset += n;
                    if self.send_offset == FRAME_BYTES {
                        if *id == self.outgoing_id + 4 {
                            self.ack_written = true;
                        }
                        self.outgoing.pop_front();
                        self.send_offset = 0;
                    } else {
                        break;
                    }
                }
                Err(quiche::Error::Done) => break,
                Err(e) => return Err(super::error(e)),
            }
        }
        if self.ack_written {
            self.ack_delivered = conn
                .stream_send_acknowledged(self.outgoing_id + 4)
                .map_err(super::error)?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "shutdown/packet_tests.rs"]
mod packet_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{config, Identity};
    use std::time::Duration;
    fn pair(window: u64) -> (quiche::Connection, quiche::Connection) {
        pair_profile(window, window, Some([7; 32]))
    }
    pub(super) fn pair_profile(
        a_window: u64,
        b_window: u64,
        psk: Option<[u8; 32]>,
    ) -> (quiche::Connection, quiche::Connection) {
        let a = Identity::generate();
        let b = Identity::generate();
        pair_identities(a_window, b_window, psk, &a, &b)
    }
    pub(super) fn pair_identities(
        a_window: u64,
        b_window: u64,
        psk: Option<[u8; 32]>,
        a: &Identity,
        b: &Identity,
    ) -> (quiche::Connection, quiche::Connection) {
        let mut ac = config(a, b.public_key(), psk, b"shutdown packet test").unwrap();
        let mut bc = config(b, a.public_key(), psk, b"shutdown packet test").unwrap();
        ac.set_initial_max_stream_data_uni(a_window);
        bc.set_initial_max_stream_data_uni(b_window);
        let aa = "127.0.0.1:1234".parse().unwrap();
        let ba = "127.0.0.1:4321".parse().unwrap();
        let mut a = quiche::connect(
            None,
            &quiche::ConnectionId::from_ref(&[1; 16]),
            aa,
            ba,
            &mut ac,
        )
        .unwrap();
        let mut b = quiche::accept(
            &quiche::ConnectionId::from_ref(&[2; 16]),
            None,
            ba,
            aa,
            &mut bc,
        )
        .unwrap();
        for _ in 0..20 {
            pump(&mut a, &mut b, &mut false);
            pump(&mut b, &mut a, &mut false);
        }
        assert!(a.is_established() && b.is_established());
        (a, b)
    }
    pub(super) fn pump(
        a: &mut quiche::Connection,
        b: &mut quiche::Connection,
        drop_one: &mut bool,
    ) {
        let mut packet = [0; 1350];
        while let Ok((n, info)) = a.send(&mut packet) {
            if std::mem::take(drop_one) {
                continue;
            }
            // Repeated encrypted packets must not become repeated control frames.
            let mut duplicate = packet[..n].to_vec();
            for bytes in [&mut packet[..n], &mut duplicate[..]] {
                match b.recv(
                    bytes,
                    quiche::RecvInfo {
                        from: info.from,
                        to: info.to,
                    },
                ) {
                    Ok(_) | Err(quiche::Error::Done) => (),
                    other => panic!("{other:?}"),
                }
            }
        }
    }
    fn timeout(c: &mut quiche::Connection) {
        if c.timeout().is_some_and(|t| t.is_zero()) {
            c.on_timeout();
        }
    }
    #[tokio::test(flavor = "current_thread")]
    async fn partial_control_frames_recover_from_loss_and_hold_crossed_closes() {
        for crossed in [false, true] {
            let (mut a, mut b) = pair(2);
            let ac = Control::new();
            let bc = Control::new();
            ac.begin(Duration::from_secs(4)).unwrap();
            if crossed {
                bc.begin(Duration::from_secs(4)).unwrap();
            }
            let mut ad = ShutdownDriver::new(ac, false);
            let mut bd = ShutdownDriver::new(bc, true);
            ad.step(&mut a, 5, false).unwrap();
            assert!(ad.protocol.sent.is_none());
            let mut drop_one = true;
            let result = tokio::time::timeout(Duration::from_secs(4), async {
                while bd.protocol.peer.is_none() {
                    ad.step(&mut a, 5, true).unwrap();
                    bd.step(&mut b, 3, true).unwrap();
                    pump(&mut a, &mut b, &mut drop_one);
                    pump(&mut b, &mut a, &mut false);
                    tokio::time::sleep(Duration::from_millis(2)).await;
                    timeout(&mut a);
                    timeout(&mut b);
                }
                assert!(!ad.acknowledged());
                bd.delivered(4).unwrap();
                bd.step(&mut b, 3, true).unwrap();
                assert!(!bd.protocol.ack_sent);
                bd.delivered(1).unwrap();
                if crossed {
                    ad.delivered(3).unwrap();
                }
                while !ad.acknowledged() || (crossed && !bd.acknowledged()) {
                    ad.step(&mut a, 5, true).unwrap();
                    bd.step(&mut b, 3, true).unwrap();
                    pump(&mut a, &mut b, &mut false);
                    pump(&mut b, &mut a, &mut false);
                    tokio::time::sleep(Duration::from_millis(2)).await;
                    timeout(&mut a);
                    timeout(&mut b);
                }
                if crossed {
                    assert!(ad.ack_delivered && bd.ack_delivered);
                }
            })
            .await;
            assert!(result.is_ok(), "crossed={crossed} a_offsets={:?}/{} b_offsets={:?}/{} a_received={:?} b_received={:?} ackwritten={}/{} ackdelivered={}/{} timeouts={:?}/{:?}", ad.receive_offset,ad.send_offset,bd.receive_offset,bd.send_offset,ad.received,bd.received,ad.ack_written,bd.ack_written,ad.ack_delivered,bd.ack_delivered,a.timeout(),b.timeout());
        }
    }
    #[tokio::test(flavor = "current_thread")]
    async fn explicit_send_receipt_survives_collection_but_rejects_resets() {
        let (mut a, mut b) = pair(128);
        assert!(a.stream_send_acknowledged(6).is_err());
        a.stream_send(6, b"receipt", true).unwrap();
        assert!(!a.stream_send_acknowledged(6).unwrap());
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                pump(&mut a, &mut b, &mut false);
                let _ = b.stream_recv(6, &mut [0; 32]);
                pump(&mut b, &mut a, &mut false);
                if a.stream_send_acknowledged(6).unwrap() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(2)).await;
                timeout(&mut a);
                timeout(&mut b);
            }
        })
        .await
        .unwrap();
        assert!(a.stream_send_acknowledged(6).unwrap());
        for stopped in [false, true] {
            let (mut a, mut b) = pair(128);
            a.stream_send(6, b"never acknowledge reset", false).unwrap();
            if stopped {
                pump(&mut a, &mut b, &mut false);
                b.stream_shutdown(6, quiche::Shutdown::Read, 42).unwrap();
                pump(&mut b, &mut a, &mut false);
                assert!(matches!(
                    a.stream_send_acknowledged(6),
                    Err(quiche::Error::StreamStopped(42))
                ));
            } else {
                a.stream_shutdown(6, quiche::Shutdown::Write, 42).unwrap();
            }
            for _ in 0..4 {
                pump(&mut a, &mut b, &mut false);
                pump(&mut b, &mut a, &mut false);
                let _ = a.stream_capacity(6); // May collect stopped state.
                assert_ne!(a.stream_send_acknowledged(6), Ok(true));
            }
        }
    }
    #[test]
    fn malformed_finished_control_streams_are_rejected_by_packet_driver() {
        for case in 0..5 {
            let (mut a, mut b) = pair(128);
            let mut bytes = Frame {
                kind: 1,
                nonce: [9; 16],
                bytes: 0,
            }
            .encode()
            .to_vec();
            let id = match case {
                0 => {
                    bytes.pop();
                    2
                }
                1 => {
                    bytes.push(0);
                    2
                }
                2 => {
                    bytes[0] = 0;
                    2
                }
                3 => {
                    bytes[4] = 2;
                    2
                }
                4 => {
                    bytes[4] = 2;
                    6
                }
                _ => unreachable!(),
            };
            a.stream_send(id, &bytes, true).unwrap();
            pump(&mut a, &mut b, &mut false);
            let mut driver = ShutdownDriver::new(Control::new(), true);
            assert!(driver.step(&mut b, 0, false).is_err(), "case {case}");
            assert!(!driver.acknowledged());
        }
    }

    #[test]
    fn crossed_receipt_and_premature_peer_close_do_not_complete_shutdown() {
        use futures::FutureExt;
        let (mut a, mut b) = pair(128);
        let control = Control::new();
        control.begin(Duration::from_secs(60)).unwrap();
        let mut driver = ShutdownDriver::new(control.clone(), false);
        driver.step(&mut a, 41, true).unwrap();
        let receipt = Frame {
            kind: 3,
            ..driver.protocol.sent.unwrap()
        };
        b.stream_send(7, &receipt.encode(), true).unwrap();
        pump(&mut b, &mut a, &mut false);
        driver.step(&mut a, 41, true).unwrap();
        assert!(driver.protocol.acknowledged);
        assert!(!driver.acknowledged());
        // Even the graceful code cannot replace the missing reciprocal request
        // and our delivery/receipt fences in a crossed exchange.
        b.close(true, ACKNOWLEDGED_CLOSE, b"shutdown acknowledged")
            .unwrap();
        pump(&mut b, &mut a, &mut false);
        assert!(a.is_draining());
        driver.finish_on_close(&a);
        drop(crate::noise_shutdown::DriverGuard(control.clone()));
        assert!(control.wait().now_or_never().unwrap().is_err());
    }
}
