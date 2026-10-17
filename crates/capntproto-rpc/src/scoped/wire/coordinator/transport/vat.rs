//! Adapt the existing message-oriented VatNetwork connection to caller-polled IO.
//! The network's own driver must still run concurrently. This bridge owns no
//! socket, executor or protocol state and never derives identity from wire IDs.
use super::*;
use capnp::capability::Promise;
use io::IoError;
use std::task::ready;

pub(crate) type Shared<V> = Rc<RefCell<Box<dyn crate::Connection<V>>>>;

/// Capture the transport identity once, then split its input, output and writer
/// failure observation. Custom transports must implement IncomingMessage's
/// get_segments and Connection's send_frame to use this migration adapter.
pub struct Connection<V> {
    connection: Shared<V>,
    id: usize,
    peer: V,
}
impl<V> Connection<V> {
    pub fn new(connection: Box<dyn crate::Connection<V>>) -> Self {
        let id = connection.connection_id();
        Self::with_identity(connection, id)
    }
    pub(crate) fn with_identity(connection: Box<dyn crate::Connection<V>>, id: usize) -> Self {
        let peer = connection.get_peer_vat_id();
        Self {
            connection: Rc::new(RefCell::new(connection)),
            id,
            peer,
        }
    }
    pub fn id(&self) -> usize {
        self.id
    }
    pub fn peer(&self) -> &V {
        &self.peer
    }
    pub(crate) fn network_connection(&self) -> Shared<V> {
        self.connection.clone()
    }
    pub(crate) fn idle_notifier(&self) -> Rc<dyn Fn(bool)>
    where
        V: 'static,
    {
        let connection = Rc::downgrade(&self.connection);
        let last = Cell::new(false);
        Rc::new(move |idle| {
            if last.get() == idle {
                return;
            }
            if let Some(connection) = connection.upgrade() {
                // A callback can reenter bootstrap. The command wake schedules
                // a fresh coordinator scan if the transport is already borrowed.
                if let Ok(mut connection) = connection.try_borrow_mut() {
                    last.set(idle);
                    connection.set_idle(idle);
                }
            }
        })
    }
    /// Bound the full input frame, including its segment table. The existing
    /// transport must also bound allocation while receiving its original message.
    /// Keep Failure polled as an abort input, including while output drains.
    pub fn into_io(self, maximum: NonZeroUsize) -> (Reader<V>, Writer<V>, Failure) {
        let (_, reader, writer, failure) = self.into_io_and_peer(maximum);
        (reader, writer, failure)
    }
    /// Transfer the captured peer identity with the IO owners. No Clone bound
    /// or second transport callback is needed by a task-owned bootstrap factory.
    pub fn into_io_and_peer(self, maximum: NonZeroUsize) -> (V, Reader<V>, Writer<V>, Failure) {
        self.connection.borrow_mut().set_idle(false);
        let finished = self.connection.borrow().when_write_finished();
        let output_driver = self.connection.borrow().output_driver();
        let input_frames = self.connection.borrow_mut().take_incoming_frames();
        let input_driver = if input_frames.is_none() {
            self.connection.borrow_mut().take_incoming_stream()
        } else {
            None
        };
        let output_stream = self.connection.borrow_mut().take_outgoing_stream();
        let closing = Rc::new(Cell::new(false));
        (
            self.peer,
            Reader {
                connection: self.connection.clone(),
                input_driver,
                input_frames,
                borrowed_frame: false,
                receiving: None,
                message: None,
                header: Vec::new(),
                position: 0,
                segment: 0,
                offset: 0,
                remaining: 0,
                maximum: maximum.get(),
                ended: false,
                #[cfg(unix)]
                fds: Vec::new(),
            },
            Writer {
                connection: self.connection,
                receipts: VecDeque::new(),
                shutdown: None,
                closing: closing.clone(),
                result: None,
                failure: None,
                output_driver,
                output_stream,
                stream_pending: 0,
                buffer_batch: false,
            },
            Failure { finished, closing },
        )
    }
}

/// Independent write-queue failure observation, without retaining the connection.
/// A successful queue completion is unexpected while open; during local close it
/// is expected and does not falsely abort an orderly shutdown.
pub struct Failure {
    finished: Option<Promise<(), Error>>,
    closing: Rc<Cell<bool>>,
}
impl Future for Failure {
    type Output = Error;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Error> {
        let Some(finished) = self.finished.as_mut() else {
            return Poll::Pending;
        };
        let result = ready!(Pin::new(finished).poll(cx));
        self.finished = None;
        match result {
            Err(error) => Poll::Ready(error),
            Ok(()) if !self.closing.get() => {
                Poll::Ready(Error::disconnected("transport write queue stopped".into()))
            }
            Ok(()) => Poll::Pending,
        }
    }
}

pub struct Reader<V> {
    connection: Shared<V>,
    input_driver: Option<Box<dyn crate::IncomingMessageStream>>,
    input_frames: Option<Box<dyn crate::IncomingFrameStream>>,
    borrowed_frame: bool,
    receiving: Option<Promise<Option<Box<dyn crate::IncomingMessage>>, Error>>,
    message: Option<Box<dyn crate::IncomingMessage>>,
    header: Vec<u8>,
    position: usize,
    segment: u32,
    offset: usize,
    remaining: usize,
    maximum: usize,
    ended: bool,
    #[cfg(unix)]
    fds: Vec<Option<Rc<std::os::fd::OwnedFd>>>,
}
impl<V> Reader<V> {
    fn start(&mut self, message: Box<dyn crate::IncomingMessage>) -> capnp::Result<()> {
        if let Some(bytes) = message.framed_bytes() {
            if bytes.len() > self.maximum {
                return Err(Error::from_kind(capnp::ErrorKind::BufferNotLargeEnough));
            }
            self.remaining = bytes.len();
            self.position = 0;
            #[cfg(unix)]
            let message = {
                let mut message = message;
                self.fds = message
                    .take_fds()
                    .into_iter()
                    .take(253)
                    .map(|fd| Some(Rc::new(fd)))
                    .collect();
                message
            };
            self.message = Some(message);
            return Ok(());
        }
        let segments = message.get_segments().ok_or_else(|| {
            Error::unimplemented("transport does not expose original RPC segments".into())
        })?;
        let count = u32::try_from(segments.len()).map_err(wire_error)?;
        let first = count
            .checked_sub(1)
            .ok_or_else(|| Error::failed("empty segment table".into()))?;
        let mut prefix = [0u8; 8];
        prefix[..4].copy_from_slice(&first.to_le_bytes());
        let table = capnp::serialize::SegmentTableHeader::parse(&prefix)?.table_bytes();
        if table > self.maximum {
            return Err(Error::from_kind(capnp::ErrorKind::BufferNotLargeEnough));
        }
        self.header.clear();
        self.header.try_reserve(table).map_err(wire_error)?;
        self.header.resize(table, 0);
        self.header[..4].copy_from_slice(&first.to_le_bytes());
        let mut total = table;
        for index in 0..count {
            let bytes = segments
                .get_segment(index)
                .ok_or_else(|| Error::failed("missing transport segment".into()))?
                .len();
            if !bytes.is_multiple_of(8) {
                return Err(Error::failed(
                    "transport segment is not a whole number of words".into(),
                ));
            }
            let words = u32::try_from(bytes / 8).map_err(wire_error)?;
            total = total
                .checked_add(bytes)
                .filter(|total| *total <= self.maximum)
                .ok_or_else(|| Error::from_kind(capnp::ErrorKind::BufferNotLargeEnough))?;
            let at = (index as usize + 1) * 4;
            self.header[at..at + 4].copy_from_slice(&words.to_le_bytes());
        }
        if (total - table) / 8 != message.size_in_words() {
            return Err(Error::failed(
                "transport message size disagrees with its segments".into(),
            ));
        }
        #[cfg(unix)]
        let message = {
            let mut message = message;
            self.fds = message
                .take_fds()
                .into_iter()
                .take(253)
                .map(|fd| Some(Rc::new(fd)))
                .collect();
            message
        };
        // No subsequent receive can replace these resources before admission.
        self.message = Some(message);
        self.position = 0;
        self.segment = 0;
        self.offset = 0;
        self.remaining = total;
        Ok(())
    }
    fn poll_message(&mut self, cx: &mut Context<'_>) -> Poll<capnp::Result<bool>> {
        if self.ended {
            return Poll::Ready(Ok(false));
        }
        if self.borrowed_frame {
            return Poll::Ready(Ok(true));
        }
        if let Some(stream) = &mut self.input_frames {
            let Some(length) = ready!(stream.poll_frame_size(cx))? else {
                self.ended = true;
                return Poll::Ready(Ok(false));
            };
            if length > self.maximum {
                return Poll::Ready(Err(Error::from_kind(
                    capnp::ErrorKind::BufferNotLargeEnough,
                )));
            }
            self.remaining = length;
            self.position = 0;
            self.borrowed_frame = true;
            return Poll::Ready(Ok(true));
        }
        if self.message.is_none() {
            if let Some(driver) = &mut self.input_driver {
                let Some(message) = ready!(driver.poll_receive(cx))? else {
                    self.ended = true;
                    return Poll::Ready(Ok(false));
                };
                self.start(message)?;
                return Poll::Ready(Ok(true));
            }
            if self.receiving.is_none() {
                let receiving = self
                    .connection
                    .borrow_mut()
                    .receive_incoming_message_short_lived();
                self.receiving = Some(receiving);
            }
            let received = ready!(Pin::new(self.receiving.as_mut().unwrap()).poll(cx));
            self.receiving = None;
            let Some(message) = received? else {
                self.ended = true;
                return Poll::Ready(Ok(false));
            };
            self.start(message)?;
        }
        Poll::Ready(Ok(true))
    }
    fn read(&mut self, cx: &mut Context<'_>, bytes: &mut [u8]) -> Poll<capnp::Result<usize>> {
        if bytes.is_empty() || !ready!(self.poll_message(cx))? {
            return Poll::Ready(Ok(0));
        }
        if self.remaining == 0 {
            return Poll::Ready(Err(Error::failed(
                "previous frame has not finished admission".into(),
            )));
        }
        let frame = if self.borrowed_frame {
            self.input_frames.as_ref().and_then(|stream| stream.frame())
        } else {
            self.message.as_ref().unwrap().framed_bytes()
        };
        if let Some(frame) = frame {
            let count = bytes.len().min(frame.len() - self.position);
            bytes[..count].copy_from_slice(&frame[self.position..self.position + count]);
            self.position += count;
            self.remaining -= count;
            return Poll::Ready(Ok(count));
        }
        if self.borrowed_frame {
            let count = ready!(self
                .input_frames
                .as_mut()
                .unwrap()
                .poll_read_frame(cx, bytes))?;
            if count > self.remaining || count > bytes.len() {
                return Poll::Ready(Err(Error::failed(
                    "stream exceeded its prepared frame".into(),
                )));
            }
            self.position += count;
            self.remaining -= count;
            return Poll::Ready(Ok(count));
        }
        let source = if self.position < self.header.len() {
            &self.header[self.position..]
        } else {
            let segments = self.message.as_ref().unwrap().get_segments().unwrap();
            loop {
                let segment = segments.get_segment(self.segment).ok_or_else(|| {
                    Error::failed("transport segments changed during read".into())
                })?;
                if self.offset < segment.len() {
                    break &segment[self.offset..];
                }
                self.segment += 1;
                self.offset = 0;
            }
        };
        let count = bytes.len().min(source.len()).min(self.remaining);
        bytes[..count].copy_from_slice(&source[..count]);
        if self.position >= self.header.len() {
            self.offset += count;
        }
        self.position += count;
        self.remaining -= count;
        Poll::Ready(Ok(count))
    }
}
impl<V> io::Read for Reader<V> {
    type Error = Error;
    fn poll_read(
        &mut self,
        cx: &mut Context<'_>,
        bytes: &mut [u8],
    ) -> Poll<Result<usize, IoError<Error>>> {
        let result = self.read(cx, bytes);
        if matches!(result, Poll::Ready(Err(_))) {
            self.ended = true;
        }
        result.map_err(IoError::Failed)
    }
}
impl<V> ImportReceiver<resources::Import> for Reader<V> {
    fn make_import(&mut self, _: ImportId) -> resources::Import {
        resources::Import::default()
    }
    fn introduced(&mut self, owner: &mut resources::Import, contact: any_pointer::Reader<'_>) {
        if owner.has_introduction() {
            return;
        }
        let result = (|| {
            contact.validate_capability_indices(0)?;
            if contact.target_size()?.word_count > self.maximum as u64 / 8 {
                return Err(Error::overloaded(
                    "introduction metadata exceeds frame budget".into(),
                ));
            }
            let mut body = capnp::message::Builder::new_default();
            body.set_root(contact)?;
            Ok(Rc::new(body))
        })();
        match result {
            Ok(body) => owner.update_metadata(|metadata| metadata.introduction = Some(body)),
            Err(error) => owner.update_metadata(|metadata| metadata.failure = Some(error)),
        }
    }
    fn attach(&mut self, owner: Option<&mut resources::Import>, index: u8) {
        #[cfg(unix)]
        {
            if index == u8::MAX {
                return;
            }
            if let Some(fd) = self.fds.get_mut(index as usize).and_then(Option::take) {
                if let Some(owner) = owner {
                    owner.attach(fd);
                }
            }
        }
        #[cfg(not(unix))]
        let _ = (owner, index);
    }
}
impl<V> Input<resources::Import> for Reader<V> {
    fn has_buffered_next_frame(&self) -> bool {
        !self.borrowed_frame
            && !self.ended
            && self
                .input_frames
                .as_ref()
                .is_some_and(|stream| stream.has_buffered_next_frame())
    }
    fn buffered_frame(&self) -> Option<&[u8]> {
        if self.borrowed_frame && self.position == 0 {
            self.input_frames.as_ref().and_then(|stream| stream.frame())
        } else {
            None
        }
    }
    fn consume_buffered_frame(&mut self) {
        self.position += self.remaining;
        self.remaining = 0;
        self.finish_frame();
    }

    fn take_buffered_frame<'p>(
        &mut self,
        messages: &mut messages::Messages<'p, '_, '_>,
        options: capnp::message::ReaderOptions,
    ) -> Result<Option<Frame<'p>>, capntproto_rpc_wire::owned::Error> {
        if self.borrowed_frame && self.position == 0 {
            let Some(bytes) = self.input_frames.as_ref().and_then(|stream| stream.frame()) else {
                return Ok(None);
            };
            let frame = Frame::copy_from(messages, bytes, options)?;
            self.position = bytes.len();
            self.remaining = 0;
            return Ok(Some(frame));
        }
        let Some(message) = self.message.as_ref() else {
            return Ok(None);
        };
        let Some(bytes) = message.framed_bytes() else {
            return Ok(None);
        };
        if self.position != 0 {
            return Ok(None);
        }
        let frame = Frame::copy_from(messages, bytes, options)?;
        if frame.size_in_words(messages)? != message.size_in_words() {
            frame.release(messages);
            return Err(capntproto_rpc_wire::owned::Error::WrongMessage);
        }
        self.position = bytes.len();
        self.remaining = 0;
        Ok(Some(frame))
    }

    fn take_shared_frame<'p>(
        &mut self,
        messages: &mut messages::Messages<'p, '_, '_>,
        options: capnp::message::ReaderOptions,
    ) -> Result<Option<Frame<'p>>, capntproto_rpc_wire::owned::Error> {
        if !self.borrowed_frame || self.position != 0 {
            return Ok(None);
        }
        let Some((allocation, range)) = self
            .input_frames
            .as_mut()
            .and_then(|stream| stream.share_frame())
        else {
            return Ok(None);
        };
        if range.len() != self.remaining {
            return Err(capntproto_rpc_wire::owned::Error::WrongMessage);
        }
        let lease = match messages.adopt_received(allocation, range) {
            Ok(Some(lease)) => lease,
            // A larger prefetched allocation may not fit even when copying its
            // single used frame would. Leave the ordinary path available.
            Ok(None) | Err(messages::Error::NoBuffer | messages::Error::GenerationExhausted) => {
                return Ok(None)
            }
            Err(error) => return Err(capntproto_rpc_wire::owned::Error::Storage(error)),
        };
        let frame = Frame::new(lease, options);
        if let Err(error) = frame.size_in_words(messages) {
            frame.release(messages);
            return Err(error);
        }
        self.position = self.remaining;
        self.remaining = 0;
        Ok(Some(frame))
    }

    fn poll_frame_size(&mut self, cx: &mut Context<'_>) -> Poll<capnp::Result<Option<usize>>> {
        let result = self
            .poll_message(cx)
            .map_ok(|_| Some(self.remaining.max(8)));
        if matches!(result, Poll::Ready(Err(_))) {
            self.ended = true;
        }
        result
    }
    fn finish_frame(&mut self) {
        // Canceling an incomplete receive is terminal. In particular, never
        // reinterpret a consumed prefix as the header of the next message.
        self.ended |= self.receiving.is_some()
            || self.remaining != 0
            || (self.input_driver.is_some() && self.message.is_none())
            || (self.input_frames.is_some() && !self.borrowed_frame);
        // Keep an unfinished transport receive alive until this reader drops.
        // Some existing transports close BOTH halves when a receive is canceled;
        // dropping it here would discard accepted output before managed drain.
        self.message = None;
        if let Some(stream) = &mut self.input_frames {
            stream.finish_frame();
        }
        self.borrowed_frame = false;
        self.header.clear();
        #[cfg(unix)]
        self.fds.clear();
    }
}

/// A bounded batch of outstanding frames. Write acceptance transfers shared
/// allocating buffers, or copies caller-backed bytes; flush waits for every
/// ordered receipt. This avoids keeping
/// a scoped pool borrow in a 'static transport task or losing detached errors.
/// Each nonempty write must contain exactly one complete standard frame. The
/// coordinator's FIFO supplies such windows; arbitrary stream chunks are rejected.
pub struct Writer<V> {
    connection: Shared<V>,
    receipts: VecDeque<Promise<(), Error>>,
    shutdown: Option<Promise<(), Error>>,
    closing: Rc<Cell<bool>>,
    result: Option<capnp::Result<()>>,
    failure: Option<Error>,
    output_driver: Option<Promise<(), Error>>,
    output_stream: Option<Box<dyn crate::OutgoingMessageStream>>,
    stream_pending: usize,
    buffer_batch: bool,
}
const MAXIMUM_RECEIPTS: usize = 32;
impl<V> Drop for Writer<V> {
    fn drop(&mut self) {
        if !self.closing.replace(true) {
            // Error/cancellation ends IO. Enqueue transport shutdown even when
            // a caller drops the coordinator before its first poll. Completion
            // is not claimed here; the originating network still owns its driver.
            let shutdown = self
                .connection
                .borrow_mut()
                .shutdown(Err(self.failure.clone().unwrap_or_else(|| {
                    Error::disconnected("portable transport IO abandoned".into())
                })));
            drop(shutdown);
        }
    }
}
impl<V> io::Flush for Writer<V> {
    type Error = Error;
    fn poll_flush(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), IoError<Error>>> {
        if let Some(error) = &self.failure {
            return Poll::Ready(Err(IoError::Failed(error.clone())));
        }
        if self.stream_pending != 0 {
            if let Some(stream) = self.output_stream.as_mut() {
                match stream.poll_flush(cx) {
                    Poll::Ready(Ok(())) => {
                        self.stream_pending = 0;
                    }
                    Poll::Ready(Err(error)) => {
                        self.failure = Some(error.clone());
                        return Poll::Ready(Err(IoError::Failed(error)));
                    }
                    Poll::Pending => (),
                }
            }
        }
        if !self.receipts.is_empty() || self.stream_pending != 0 {
            if let Some(driver) = self.output_driver.as_mut() {
                if let Poll::Ready(result) = Pin::new(driver).poll(cx) {
                    self.output_driver = None;
                    if let Err(error) = result {
                        self.failure = Some(error.clone());
                        return Poll::Ready(Err(IoError::Failed(error)));
                    }
                }
            }
        }
        if self.stream_pending != 0 {
            if let Some(stream) = self.output_stream.as_mut() {
                if let Err(error) = ready!(stream.poll_flush(cx)) {
                    self.failure = Some(error.clone());
                    return Poll::Ready(Err(IoError::Failed(error)));
                }
                self.stream_pending = 0;
            }
        }
        while let Some(receipt) = self.receipts.front_mut() {
            let result = ready!(Pin::new(receipt).poll(cx));
            self.receipts.pop_front();
            if let Err(error) = result {
                self.failure = Some(error.clone());
                return Poll::Ready(Err(IoError::Failed(error)));
            }
        }
        self.buffer_batch = false;
        Poll::Ready(Ok(()))
    }
}
impl<V> io::Close for Writer<V> {
    fn poll_close(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), IoError<Error>>> {
        if let Some(result) = &self.result {
            return Poll::Ready(result.clone().map_err(IoError::Failed));
        }
        ready!(io::Flush::poll_flush(self, cx))?;
        if self.shutdown.is_none() {
            self.closing.set(true);
            let shutdown = self.connection.borrow_mut().shutdown(Ok(()));
            self.shutdown = Some(shutdown);
        }
        let result = ready!(Pin::new(self.shutdown.as_mut().unwrap()).poll(cx));
        self.shutdown = None;
        self.result = Some(result.clone());
        Poll::Ready(result.map_err(IoError::Failed))
    }
}

#[derive(Default)]
pub struct Bundle {
    #[cfg(unix)]
    fds: Option<Box<capntproto_rpc_wire::attachments::Sender<Rc<std::os::fd::OwnedFd>, 4>>>,
    credit: Option<Rc<crate::admission::Permit>>,
}

impl<V> io::WriteResources<Bundle> for Writer<V> {
    fn accepts_parts(&self) -> bool {
        true
    }
    fn poll_write_parts(
        &mut self,
        cx: &mut Context<'_>,
        parts: messages::SharedParts,
        resources: Option<&Bundle>,
    ) -> Poll<Result<usize, IoError<Error>>> {
        ready!(self.poll_capacity(cx))?;
        let length = parts.len();
        let frame = crate::OutgoingFrame::from_parts(parts).map_err(IoError::Failed)?;
        self.enqueue(frame, resources)?;
        Poll::Ready(Ok(length))
    }
    fn start_batch(&mut self, frames: usize) {
        self.buffer_batch = frames > 1;
    }
    fn accepts_shared(&self) -> bool {
        true
    }
    fn poll_write_shared(
        &mut self,
        cx: &mut Context<'_>,
        bytes: messages::SharedBytes,
        resources: Option<&Bundle>,
    ) -> Poll<Result<usize, IoError<Error>>> {
        ready!(self.poll_capacity(cx))?;
        let length = bytes.len();
        if length == 0 {
            return Poll::Ready(Ok(0));
        }
        let frame = crate::OutgoingFrame::from_shared(bytes).map_err(IoError::Failed)?;
        self.enqueue(frame, resources)?;
        Poll::Ready(Ok(length))
    }
    fn poll_write_resources(
        &mut self,
        cx: &mut Context<'_>,
        bytes: &[u8],
        resources: Option<&Bundle>,
    ) -> Poll<Result<usize, IoError<Error>>> {
        ready!(self.poll_capacity(cx))?;
        if bytes.is_empty() {
            return Poll::Ready(Ok(0));
        }
        let frame = crate::OutgoingFrame::copy_from(bytes).map_err(IoError::Failed)?;
        self.enqueue(frame, resources)?;
        Poll::Ready(Ok(bytes.len()))
    }
}
impl<V> Writer<V> {
    fn poll_capacity(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), IoError<Error>>> {
        if self.closing.get() {
            return Poll::Ready(Err(IoError::Failed(Error::disconnected(
                "transport closing".into(),
            ))));
        }
        if let Some(error) = &self.failure {
            return Poll::Ready(Err(IoError::Failed(error.clone())));
        }
        if self.receipts.len() + self.stream_pending == MAXIMUM_RECEIPTS {
            ready!(io::Flush::poll_flush(self, cx))?;
        }
        Poll::Ready(Ok(()))
    }
    #[inline(always)]
    fn enqueue(
        &mut self,
        mut frame: crate::OutgoingFrame,
        resources: Option<&Bundle>,
    ) -> Result<(), IoError<Error>> {
        frame.credit = resources.and_then(|resources| resources.credit.clone());
        #[cfg(unix)]
        let frame = {
            let mut frame = frame;
            if let Some(fds) = resources.and_then(|resources| resources.fds.as_ref()) {
                frame.set_fds(fds.iter().cloned().collect());
            }
            frame
        };
        #[cfg(not(unix))]
        let _ = resources;
        if let Some(stream) = self.output_stream.as_mut() {
            // OutgoingMessageStream retains the frame and its resources until
            // physical completion, even if this observer is dropped. The frame
            // already owns admission credit; a second owner/list is redundant.
            if self.buffer_batch {
                stream.send_buffered(frame)
            } else {
                stream.send(frame)
            }
            .map_err(IoError::Failed)?;
            self.stream_pending += 1;
            return Ok(());
        }
        self.receipts
            .try_reserve(1)
            .map_err(|error| IoError::Failed(wire_error(error)))?;
        let credit = frame.credit.clone();
        let receipt = self.connection.borrow_mut().send_frame(frame);
        use crate::attach::Attach;
        self.receipts.push_back(match credit {
            Some(credit) => Promise::from_future(receipt.attach(credit)),
            None => receipt,
        });
        // This is transport queue acceptance; flush supplies physical completion.
        Ok(())
    }
}

/// Attachment policy for the existing VatNetwork transports. Byte-only
/// transports explicitly discard optional descriptors at their send boundary.
pub struct Resources {
    #[cfg(unix)]
    maximum: usize,
}
impl Resources {
    pub fn new(maximum_fds: usize) -> Self {
        #[cfg(not(unix))]
        let _ = maximum_fds;
        Self {
            #[cfg(unix)]
            maximum: maximum_fds.min(253),
        }
    }
    #[cfg(unix)]
    fn attach_fd(&self, bundle: &mut Bundle, fd: Option<Rc<std::os::fd::OwnedFd>>) -> u8 {
        let Some(fd) = fd.filter(|_| self.maximum != 0) else {
            return u8::MAX;
        };
        // Plain TCP/QUIC calls carry only their admission permit. Allocate
        // attachment storage only once a real resource is actually published.
        bundle
            .fds
            .get_or_insert_with(|| {
                Box::new(capntproto_rpc_wire::attachments::Sender::new(
                    Capacity::Growing {
                        maximum: self.maximum,
                    },
                ))
            })
            .push(Some(fd))
    }
}
impl Default for Resources {
    fn default() -> Self {
        Self::new(253)
    }
}
impl Output<resources::Import> for Resources {
    type Bundle = Bundle;
    fn bundle(&self) -> Bundle {
        Bundle::default()
    }
    fn retain_call(
        &self,
        bundle: &mut Bundle,
        credit: Rc<crate::admission::Permit>,
    ) -> capnp::Result<()> {
        bundle.credit = Some(credit);
        Ok(())
    }
    fn attach<'id, P: Copy, const H: usize>(
        &self,
        owner: &Route<'id, P, Rc<dyn ClientHook>, H>,
        imports: &Imports<'id, '_, resources::Import>,
        bundle: &mut Bundle,
    ) -> u8 {
        #[cfg(unix)]
        {
            self.attach_fd(
                bundle,
                match owner {
                    Route::Capability(Endpoint::Imported(remote)) => {
                        imports.get(remote.reference()).metadata().fd
                    }
                    Route::Capability(Endpoint::Local(hook)) | Route::Vine(hook) => hook.get_fd(),
                    _ => None,
                },
            )
        }
        #[cfg(not(unix))]
        {
            let _ = (owner, imports, bundle);
            u8::MAX
        }
    }
    fn metadata_source(&self, owner: &resources::Import) -> Option<Rc<MetadataSource>> {
        Some(owner.metadata_source())
    }
}

#[cfg(test)]
mod tests;
