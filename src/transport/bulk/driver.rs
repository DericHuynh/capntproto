//! The socket task owns every Quiche operation. Application handles use bounded
//! local pipes and never run user wakers while the admission state is borrowed.
use super::*;
use std::task::{Wake, Waker};

const RESET: u64 = 0x435442;
const ACK: usize = 9;
const ACK_INTERVAL: u64 = 8 * 1024;

struct WakeDriver(Arc<Notify>);
impl Wake for WakeDriver {
    fn wake(self: Arc<Self>) {
        self.0.notify_one();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.notify_one();
    }
}

pub(crate) struct Driver {
    shared: Rc<RefCell<Shared>>,
    initial_cid: Vec<u8>,
    identities: [[u8; 32]; 2],
    server: bool,
    next_send: u64,
    initialized: bool,
    limit: u64,
    // Unconfirmed bytes from failed transfers stay charged for this session.
    // Cancellation cannot repeatedly spend credit reserved for control RPC.
    abandoned: u64,
    entries: Vec<Entry>,
    prefaces: Vec<Preface>,
    waker: Waker,
}
pub(super) struct Entry {
    offer: Offer,
    id: Option<u64>,
    progress: Rc<Progress>,
    deadline: Instant,
    direction: Direction,
}
enum Direction {
    Send(Producer),
    Receive(Consumer),
}
struct Producer {
    read: local_io::ReadHalf,
    header: [u8; wire::HEADER],
    header_sent: usize,
    buffer: Box<[u8; BUFFER]>,
    start: usize,
    end: usize,
    sent: u64,
    fin: bool,
    ack: [u8; ACK],
    ack_used: usize,
    confirmed: Option<u64>,
    final_ack: bool,
    received_fin: bool,
}
struct Consumer {
    write: Option<local_io::WriteHalf>,
    buffer: Box<[u8; BUFFER]>,
    start: usize,
    end: usize,
    received: u64,
    fin: bool,
    ack: [u8; ACK],
    ack_sent: usize,
    acknowledged: Option<u64>,
    final_ack: bool,
}
struct Preface {
    id: u64,
    bytes: [u8; wire::HEADER],
    used: usize,
    deadline: Instant,
}

pub(crate) fn pair(
    conn: &quiche::Connection<impl quiche::BufFactory>,
    local: [u8; 32],
    peer: [u8; 32],
    control: crate::native_shutdown::Control,
    wake: Arc<Notify>,
) -> (Plane, Driver) {
    let server = conn.is_server();
    let shared = Rc::new(RefCell::new(Shared {
        control,
        live: true,
        closing: false,
        binding: None,
        next_receive: u64::from(server),
        server,
        sent_ids: wire::IdWindow::default(),
        active: 0,
        pending: Vec::new(),
        wake: wake.clone(),
        stats: Stats::default(),
    }));
    let driver = Driver {
        shared: shared.clone(),
        initial_cid: conn.source_id().to_vec(),
        identities: if server { [peer, local] } else { [local, peer] },
        server,
        next_send: if server { 1 } else { 4 },
        initialized: false,
        limit: 0,
        abandoned: 0,
        entries: Vec::new(),
        prefaces: Vec::new(),
        waker: Waker::from(Arc::new(WakeDriver(wake))),
    };
    (Plane(Rc::downgrade(&shared)), driver)
}
impl Driver {
    pub fn deadline(&self) -> Option<Instant> {
        self.entries
            .iter()
            .map(|e| e.deadline)
            .chain(self.prefaces.iter().map(|p| p.deadline))
            .min()
    }
    pub fn drained(&self) -> bool {
        self.shared.borrow().active == 0 && self.prefaces.is_empty()
    }
    pub fn close_admission(&self) {
        self.shared.borrow_mut().closing = true;
    }
    fn initialize(
        &mut self,
        conn: &mut quiche::Connection<impl quiche::BufFactory>,
    ) -> io::Result<()> {
        if self.initialized {
            return Ok(());
        }
        self.initialized = true;
        let params = conn.peer_transport_params().ok_or_else(closed)?;
        let peer_cid = params
            .initial_source_connection_id
            .as_ref()
            .ok_or_else(closed)?;
        if params.initial_max_data < 2 * CONTROL_RESERVE {
            return Ok(());
        }
        self.limit = MAX_OUTSTANDING.min(params.initial_max_data / 2);
        let mut hash = ring::digest::Context::new(&ring::digest::SHA256);
        hash.update(b"capntproto/3 bulk plane 1\0");
        for identity in self.identities {
            hash.update(&identity);
        }
        let cids: [&[u8]; 2] = if self.server {
            [peer_cid.as_ref(), &self.initial_cid]
        } else {
            [&self.initial_cid, peer_cid.as_ref()]
        };
        for cid in cids {
            hash.update(&(cid.len() as u64).to_be_bytes());
            hash.update(cid);
        }
        self.shared.borrow_mut().binding = Some(hash.finish().as_ref().try_into().unwrap());
        conn.stream_priority(0, 0, false)
            .map_err(crate::transport::error)
    }
    #[inline]
    pub fn step(
        &mut self,
        conn: &mut quiche::Connection<impl quiche::BufFactory>,
    ) -> io::Result<()> {
        // RPC usually drains the only readable stream before calling us. Keep
        // inactive bulk work out of its hot path, including initialization's
        // stack frame and the readable-stream snapshot. Newly admitted grants
        // must still run even when the peer has not sent any stream data yet.
        if self.initialized
            && self.entries.is_empty()
            && self.prefaces.is_empty()
            && self.shared.borrow().pending.is_empty()
            && !conn.is_readable()
        {
            return Ok(());
        }
        self.step_streams(conn)
    }

    #[inline(never)]
    fn step_streams(
        &mut self,
        conn: &mut quiche::Connection<impl quiche::BufFactory>,
    ) -> io::Result<()> {
        self.initialize(conn)?;
        // Move entries without discarding the bounded admission queue's storage.
        // The borrow ends before driving pipes or waking application handles.
        self.entries.append(&mut self.shared.borrow_mut().pending);
        // Do not read a clock on the ordinary RPC-only fast path.
        // Quiche already returns an owned snapshot. Inspect it directly instead
        // of allocating another stream-ID list on every packet/control event.
        let mut readable = conn
            .readable()
            .filter(|id| *id != 0 && *id % 4 < 2)
            .peekable();
        if self.entries.is_empty() && readable.peek().is_none() && self.prefaces.is_empty() {
            return Ok(());
        }
        let now = Instant::now();
        for id in readable {
            if self.entries.iter().any(|e| e.id == Some(id))
                || self.prefaces.iter().any(|p| p.id == id)
            {
                continue;
            }
            if id % 2 == u64::from(self.server)
                || self.prefaces.len() == MAX_ACTIVE
                || self.limit == 0
            {
                reset(conn, id);
            } else {
                self.prefaces.push(Preface {
                    id,
                    bytes: [0; wire::HEADER],
                    used: 0,
                    deadline: now + Duration::from_secs(5),
                });
            }
        }
        let mut i = 0;
        while i < self.prefaces.len() {
            let p = &mut self.prefaces[i];
            let result = if p.deadline <= now {
                Err(invalid("bulk preface expired"))
            } else {
                match conn.stream_recv(p.id, &mut p.bytes[p.used..]) {
                    Ok((n, fin)) => {
                        p.used += n;
                        Ok(Some(fin))
                    }
                    Err(quiche::Error::Done) => Ok(None),
                    Err(e) => Err(crate::transport::error(e)),
                }
            };
            let complete = p.used == wire::HEADER;
            if !complete && matches!(result, Ok(None | Some(false))) {
                i += 1;
                continue;
            }
            let p = self.prefaces.swap_remove(i);
            let grant = u64::from_be_bytes(p.bytes[40..48].try_into().unwrap());
            let valid = self.entries.iter_mut().find(|e| {
                e.id.is_none()
                    && matches!(e.direction, Direction::Receive(_))
                    && e.offer.id == grant
            });
            let accepted = match (result, valid) {
                (Ok(Some(fin)), Some(e))
                    if complete
                        && e.offer.validate(&p.bytes).is_ok()
                        && (!fin || e.offer.length == 0) =>
                {
                    e.id = Some(p.id);
                    if let Direction::Receive(c) = &mut e.direction {
                        c.fin = fin;
                    }
                    conn.stream_priority(p.id, 8, true)
                        .map_err(crate::transport::error)?;
                    true
                }
                _ => false,
            };
            if !accepted {
                reset(conn, p.id);
            }
        }
        let waker = self.waker.clone();
        let mut cx = Context::from_waker(&waker);
        let mut outstanding =
            self.abandoned + self.entries.iter().map(Entry::outstanding).sum::<u64>();
        let mut i = 0;
        while i < self.entries.len() {
            let e = &mut self.entries[i];
            let previous = e.outstanding();
            let (sent_before, received_before) = e.bytes();
            let result = if e.progress.canceled.get() {
                Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "bulk transfer canceled",
                ))
            } else if now >= e.deadline {
                Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "bulk transfer deadline elapsed",
                ))
            } else {
                let admitted = if e.id.is_none() && matches!(e.direction, Direction::Send(_)) {
                    match if self.next_send < (1 << 62) {
                        conn.stream_priority(self.next_send, 200, true)
                    } else {
                        Err(quiche::Error::InvalidStreamState(self.next_send))
                    } {
                        Ok(()) => {
                            e.id = Some(self.next_send);
                            self.next_send += 4;
                            Ok(true)
                        }
                        Err(quiche::Error::StreamLimit) => Ok(false),
                        Err(error) => Err(crate::transport::error(error)),
                    }
                } else {
                    Ok(true)
                };
                match admitted {
                    Ok(true) => e.step(
                        conn,
                        &mut cx,
                        self.limit
                            .saturating_sub(outstanding)
                            .min((self.limit / MAX_ACTIVE as u64).saturating_sub(previous)),
                    ),
                    Ok(false) => Ok(false),
                    Err(e) => Err(e),
                }
            };
            outstanding = outstanding - previous + e.outstanding();
            let (sent_after, received_after) = e.bytes();
            {
                let mut shared = self.shared.borrow_mut();
                shared.stats.sent_bytes = shared
                    .stats
                    .sent_bytes
                    .saturating_add(sent_after - sent_before);
                shared.stats.received_bytes = shared
                    .stats
                    .received_bytes
                    .saturating_add(received_after - received_before);
            }
            match result {
                Ok(false) => i += 1,
                result => {
                    let e = self.entries.swap_remove(i);
                    self.shared.borrow_mut().active -= 1;
                    if let Err(error) = result {
                        self.abandoned += e.outstanding();
                        if let Some(id) = e.id {
                            reset(conn, id);
                        }
                        e.progress.finish(Err(error));
                    } else {
                        e.progress.finish(Ok(()));
                    }
                    // Dropping pipe halves wakes the application outside Shared.
                }
            }
        }
        let mut shared = self.shared.borrow_mut();
        shared.stats.outstanding_bytes = outstanding;
        shared.stats.abandoned_bytes = self.abandoned;
        Ok(())
    }
}
impl Drop for Driver {
    fn drop(&mut self) {
        let pending = {
            let mut s = self.shared.borrow_mut();
            s.live = false;
            std::mem::take(&mut s.pending)
        };
        for e in self.entries.iter().chain(pending.iter()) {
            e.progress.finish(Err(closed()));
        }
    }
}
fn reset(conn: &mut quiche::Connection<impl quiche::BufFactory>, id: u64) {
    let _ = conn.stream_shutdown(id, quiche::Shutdown::Read, RESET);
    let _ = conn.stream_shutdown(id, quiche::Shutdown::Write, RESET);
}
impl Entry {
    fn bytes(&self) -> (u64, u64) {
        match &self.direction {
            Direction::Send(p) => (p.sent, 0),
            Direction::Receive(c) => (0, c.received),
        }
    }
    pub fn send(
        offer: Offer,
        read: local_io::ReadHalf,
        progress: Rc<Progress>,
        deadline: Instant,
    ) -> Self {
        let header = offer.header();
        Self {
            offer,
            id: None,
            progress,
            deadline,
            direction: Direction::Send(Producer {
                read,
                header,
                header_sent: 0,
                buffer: Box::new([0; BUFFER]),
                start: 0,
                end: 0,
                sent: 0,
                fin: false,
                ack: [0; ACK],
                ack_used: 0,
                confirmed: None,
                final_ack: false,
                received_fin: false,
            }),
        }
    }
    pub fn receive(
        offer: &Offer,
        write: local_io::WriteHalf,
        progress: Rc<Progress>,
        deadline: Instant,
    ) -> Self {
        Self {
            offer: Offer::decode(&offer.encode()).expect("locally constructed offer"),
            id: None,
            progress,
            deadline,
            direction: Direction::Receive(Consumer {
                write: Some(write),
                buffer: Box::new([0; BUFFER]),
                start: 0,
                end: 0,
                received: 0,
                fin: false,
                ack: [0; ACK],
                ack_sent: ACK,
                acknowledged: None,
                final_ack: false,
            }),
        }
    }
    fn outstanding(&self) -> u64 {
        match &self.direction {
            Direction::Send(p) => {
                p.sent - p.confirmed.unwrap_or(0)
                    + if p.confirmed.is_none() {
                        p.header_sent as u64
                    } else {
                        0
                    }
            }
            _ => 0,
        }
    }
    fn step(
        &mut self,
        conn: &mut quiche::Connection<impl quiche::BufFactory>,
        cx: &mut Context<'_>,
        credit: u64,
    ) -> io::Result<bool> {
        let Some(id) = self.id else {
            return Ok(false);
        };
        match &mut self.direction {
            Direction::Send(p) => p.step(conn, id, self.offer.length, &self.progress, cx, credit),
            Direction::Receive(c) => c.step(conn, id, self.offer.length, &self.progress, cx),
        }
    }
}

impl Producer {
    fn step(
        &mut self,
        conn: &mut quiche::Connection<impl quiche::BufFactory>,
        id: u64,
        length: u64,
        progress: &Progress,
        cx: &mut Context<'_>,
        mut credit: u64,
    ) -> io::Result<bool> {
        if self.received_fin {
            return match conn.stream_capacity(id) {
                Err(quiche::Error::InvalidStreamState(_)) => Ok(true),
                Err(e) => Err(crate::transport::error(e)),
                _ => Ok(false),
            };
        }
        // Read at most one bounded batch; coalesced receipts never allocate.
        for _ in 0..32 {
            match conn.stream_recv(id, &mut self.ack[self.ack_used..]) {
                Ok((n, fin)) => {
                    if self.final_ack && (n != 0 || !fin) {
                        return Err(invalid("data after final bulk receipt"));
                    }
                    self.ack_used += n;
                    if self.ack_used == ACK {
                        let count = u64::from_be_bytes(self.ack[1..].try_into().unwrap());
                        if self.header_sent != wire::HEADER
                            || count < self.confirmed.unwrap_or(0)
                            || count > self.sent
                            || !matches!(self.ack[0], b'A' | b'F')
                            || (self.ack[0] == b'F' && (count != length || !self.fin))
                        {
                            return Err(invalid("invalid bulk receipt"));
                        }
                        credit += count - self.confirmed.unwrap_or(0)
                            + if self.confirmed.is_none() {
                                self.header_sent as u64
                            } else {
                                0
                            };
                        self.confirmed = Some(count);
                        self.final_ack = self.ack[0] == b'F';
                        self.ack_used = 0;
                    }
                    if fin {
                        if !self.final_ack || self.ack_used != 0 {
                            return Err(invalid("truncated bulk receipt"));
                        }
                        self.received_fin = true;
                        progress.finish(Ok(()));
                        // Wait for Quiche to collect the fully acknowledged
                        // stream before allowing graceful connection closure.
                        progress.wake.notify_one();
                        return Ok(false);
                    }
                    if n == 0 {
                        break;
                    }
                }
                Err(quiche::Error::Done | quiche::Error::InvalidStreamState(_)) => break,
                Err(e) => return Err(crate::transport::error(e)),
            }
        }
        if self.header_sent < wire::HEADER {
            let end = wire::HEADER.min(self.header_sent + credit as usize);
            if end == self.header_sent {
                return Ok(false);
            }
            match conn.stream_send(id, &self.header[self.header_sent..end], false) {
                Ok(n) => {
                    self.header_sent += n;
                    credit -= n as u64;
                }
                Err(quiche::Error::Done | quiche::Error::StreamLimit) => return Ok(false),
                Err(e) => return Err(crate::transport::error(e)),
            }
            if self.header_sent != wire::HEADER {
                return Ok(false);
            }
        }
        if self.fin {
            return Ok(false);
        }
        if self.start == self.end {
            let mut buf = ReadBuf::new(&mut *self.buffer);
            match Pin::new(&mut self.read).poll_read(cx, &mut buf) {
                Poll::Pending => return Ok(false),
                Poll::Ready(Err(e)) => return Err(e),
                Poll::Ready(Ok(())) => {
                    self.start = 0;
                    self.end = buf.filled().len();
                    if self.end == 0 {
                        if self.sent != length {
                            return Err(invalid("incomplete bulk producer"));
                        }
                        match conn.stream_send(id, &[], true) {
                            Ok(_) => self.fin = true,
                            Err(quiche::Error::Done) => (),
                            Err(e) => return Err(crate::transport::error(e)),
                        }
                        return Ok(false);
                    }
                }
            }
        }
        let end = self.end.min(self.start + credit as usize);
        if end == self.start {
            return Ok(false);
        }
        match conn.stream_send(id, &self.buffer[self.start..end], false) {
            Ok(n) => {
                self.start += n;
                self.sent += n as u64;
                if n > 0 {
                    progress.wake.notify_one();
                }
            }
            Err(quiche::Error::Done) => (),
            Err(e) => return Err(crate::transport::error(e)),
        }
        Ok(false)
    }
}
impl Consumer {
    fn step(
        &mut self,
        conn: &mut quiche::Connection<impl quiche::BufFactory>,
        id: u64,
        length: u64,
        progress: &Progress,
        cx: &mut Context<'_>,
    ) -> io::Result<bool> {
        if self.final_ack && self.ack_sent == ACK {
            return match conn.stream_capacity(id) {
                Err(quiche::Error::InvalidStreamState(_)) => Ok(true),
                Err(e) => Err(crate::transport::error(e)),
                _ => Ok(false),
            };
        }
        if self.start == self.end && !self.fin {
            match conn.stream_recv(id, &mut *self.buffer) {
                Ok((n, fin)) => {
                    self.received += n as u64;
                    if self.received > length || (fin && self.received != length) {
                        return Err(invalid("bulk stream length mismatch"));
                    }
                    self.start = 0;
                    self.end = n;
                    self.fin = fin;
                }
                Err(quiche::Error::Done) => (),
                Err(e) => return Err(crate::transport::error(e)),
            }
        }
        if self.start < self.end {
            match Pin::new(self.write.as_mut().unwrap())
                .poll_write(cx, &self.buffer[self.start..self.end])
            {
                Poll::Ready(Ok(0)) => return Err(closed()),
                Poll::Ready(Ok(n)) => {
                    self.start += n;
                    progress.wake.notify_one();
                }
                Poll::Ready(Err(e)) => return Err(e),
                Poll::Pending => (),
            }
        }
        if self.fin && self.start == self.end {
            progress.input_finished.set(true);
            self.write.take();
        }
        let consumed = progress.consumed.get();
        let final_ack = progress.input_finished.get() && consumed == length;
        if self.ack_sent == ACK
            && !self.final_ack
            && (final_ack
                || self.acknowledged.is_none()
                || consumed - self.acknowledged.unwrap() >= ACK_INTERVAL)
        {
            self.ack[0] = if final_ack { b'F' } else { b'A' };
            self.ack[1..].copy_from_slice(&consumed.to_be_bytes());
            self.ack_sent = 0;
            self.acknowledged = Some(consumed);
            self.final_ack = final_ack;
        }
        if self.ack_sent < ACK {
            match conn.stream_send(id, &self.ack[self.ack_sent..], self.final_ack) {
                Ok(n) => {
                    self.ack_sent += n;
                    if n > 0 {
                        progress.wake.notify_one();
                    }
                }
                Err(quiche::Error::Done) => (),
                Err(e) => return Err(crate::transport::error(e)),
            }
        }
        Ok(false)
    }
}
