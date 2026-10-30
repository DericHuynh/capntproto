//! Host one exclusive frame transport with a single end-of-turn flush owner.
//! The network's own driver must still run concurrently. This bridge owns no
//! socket, executor or protocol state and never derives identity from wire IDs.
use super::*;
use capnp::capability::Promise;
use io::IoError;
use std::task::ready;

pub(crate) type Shared<V> = Rc<RefCell<Box<dyn crate::Connection<V>>>>;

/// Capture the transport identity once, then split its input, output and writer
/// failure observation. Frame ownership is selected once at construction.
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
    pub fn into_io(self, maximum: NonZeroUsize) -> capnp::Result<(Reader, Writer, Failure)> {
        let (_, reader, writer, failure) = self.into_io_and_peer(maximum)?;
        Ok((reader, writer, failure))
    }
    pub fn into_io_and_peer(
        self,
        maximum: NonZeroUsize,
    ) -> capnp::Result<(V, Reader, Writer, Failure)> {
        self.connection.borrow_mut().set_idle(false);
        let crate::FrameIo {
            incoming,
            outgoing,
            write_finished,
        } = self.connection.borrow_mut().take_frame_io()?;
        let closing = Rc::new(messages::OutputStatus::new(MAXIMUM_PENDING_FRAMES));
        Ok((
            self.peer,
            Reader {
                input: incoming,
                active: false,
                position: 0,
                remaining: 0,
                maximum: maximum.get(),
                ended: false,
                #[cfg(unix)]
                fds: Vec::new(),
                #[cfg(unix)]
                fds_loaded: false,
            },
            Writer {
                shutdown: None,
                closing: closing.clone(),
                result: None,
                state: Rc::new(RefCell::new(OutputState {
                    output: outgoing,
                    failure: None,
                })),
            },
            Failure {
                finished: write_finished,
                closing,
            },
        ))
    }
}

/// Independent write-queue failure observation, without retaining the connection.
/// A successful queue completion is unexpected while open; during local close it
/// is expected and does not falsely abort an orderly shutdown.
pub struct Failure {
    finished: Option<Promise<(), Error>>,
    closing: Rc<messages::OutputStatus>,
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
            Ok(()) if !self.closing.is_closed() => {
                Poll::Ready(Error::disconnected("transport write queue stopped".into()))
            }
            Ok(()) => Poll::Pending,
        }
    }
}

pub struct Reader {
    input: Box<dyn crate::IncomingFrameStream>,
    active: bool,
    position: usize,
    remaining: usize,
    maximum: usize,
    ended: bool,
    #[cfg(unix)]
    fds: Vec<Option<Rc<std::os::fd::OwnedFd>>>,
    #[cfg(unix)]
    fds_loaded: bool,
}
impl Reader {
    fn poll_message(&mut self, cx: &mut Context<'_>) -> Poll<capnp::Result<bool>> {
        if self.ended {
            return Poll::Ready(Ok(false));
        }
        if self.active {
            return Poll::Ready(Ok(true));
        }
        let Some(length) = ready!(self.input.poll_frame_size(cx))? else {
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
        self.active = true;
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
        let count = if let Some(frame) = self.input.frame() {
            let source = frame
                .get(self.position..)
                .ok_or_else(|| Error::failed("frame changed during admission".into()))?;
            let count = bytes.len().min(source.len()).min(self.remaining);
            bytes[..count].copy_from_slice(&source[..count]);
            count
        } else {
            let count = ready!(self.input.poll_read_frame(cx, bytes))?;
            if count > self.remaining || count > bytes.len() {
                return Poll::Ready(Err(Error::failed(
                    "stream exceeded its prepared frame".into(),
                )));
            }
            count
        };
        self.position += count;
        self.remaining -= count;
        Poll::Ready(Ok(count))
    }
    #[cfg(unix)]
    fn load_fds(&mut self) {
        if !self.fds_loaded {
            self.fds_loaded = true;
            self.fds = self
                .input
                .take_fds()
                .into_iter()
                .take(253)
                .map(|fd| Some(Rc::new(fd)))
                .collect();
        }
    }
}
impl io::Read for Reader {
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
impl ImportReceiver<resources::Import> for Reader {
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
            self.load_fds();
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
impl Input<resources::Import> for Reader {
    fn buffered_next_frame_size(&mut self) -> Option<NonZeroUsize> {
        if self.active || self.ended {
            None
        } else {
            self.input.buffered_next_frame_size()
        }
    }
    fn buffered_frame(&self) -> Option<&[u8]> {
        if self.active && self.position == 0 {
            self.input.frame()
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
        if !self.active || self.position != 0 {
            return Ok(None);
        }
        let Some(bytes) = self.input.frame() else {
            return Ok(None);
        };
        if bytes.len() != self.remaining {
            return Err(capntproto_rpc_wire::owned::Error::WrongMessage);
        }
        // Admission immediately decodes this immutable owner under `options`.
        // Parsing its segment table here as well duplicates that validation,
        // including another pool lookup and borrow for every small Call.
        let lease = messages
            .copy_from(bytes)
            .map_err(capntproto_rpc_wire::owned::Error::Storage)?;
        let frame = Frame::new(lease, options);
        self.position = bytes.len();
        self.remaining = 0;
        Ok(Some(frame))
    }
    fn take_shared_frame<'p>(
        &mut self,
        messages: &mut messages::Messages<'p, '_, '_>,
        options: capnp::message::ReaderOptions,
    ) -> Result<Option<Frame<'p>>, capntproto_rpc_wire::owned::Error> {
        if !self.active || self.position != 0 {
            return Ok(None);
        }
        let Some((allocation, range)) = self.input.share_frame() else {
            return Ok(None);
        };
        if range.len() != self.remaining {
            return Err(capntproto_rpc_wire::owned::Error::WrongMessage);
        }
        let lease = match messages.adopt_received(allocation, range) {
            Ok(Some(lease)) => lease,
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
        // EOF still needs an eight-byte reservation so the byte reader can
        // report zero without reserving the configured maximum frame size.
        let result = self
            .poll_message(cx)
            .map_ok(|_| Some(self.remaining.max(8)));
        if matches!(result, Poll::Ready(Err(_))) {
            self.ended = true;
        }
        result
    }
    fn finish_frame(&mut self) {
        self.ended |= self.remaining != 0 || !self.active;
        self.input.finish_frame();
        self.active = false;
        #[cfg(unix)]
        {
            self.fds.clear();
            self.fds_loaded = false;
        }
    }
}

/// A bounded batch of outstanding frames. Write acceptance transfers shared
/// allocating buffers, or copies caller-backed bytes; flush waits for every
/// accepted frame. This avoids keeping
/// a scoped pool borrow in a 'static transport task or losing detached errors.
/// Each nonempty write must contain exactly one complete standard frame. The
/// coordinator's FIFO supplies such windows; arbitrary stream chunks are rejected.
pub struct Writer {
    shutdown: Option<Promise<(), Error>>,
    closing: Rc<messages::OutputStatus>,
    result: Option<capnp::Result<()>>,
    state: Rc<RefCell<OutputState>>,
}
struct OutputState {
    output: Box<dyn crate::OutgoingMessageStream>,
    failure: Option<Error>,
}
// Standalone frame adapters bound their output independently. Hosted connections
// set the physical publication limit from their configured output queue.
const MAXIMUM_PENDING_FRAMES: usize = 64;
impl Writer {
    fn poll_shutdown(
        &mut self,
        cx: &mut Context<'_>,
        reason: capnp::Result<()>,
    ) -> Poll<Result<(), IoError<Error>>> {
        if let Some(result) = &self.result {
            return Poll::Ready(result.clone().map_err(IoError::Failed));
        }
        if self.shutdown.is_none() {
            self.closing.close();
            self.shutdown = Some(self.state.borrow_mut().output.shutdown(reason));
        }
        let result = ready!(Pin::new(self.shutdown.as_mut().unwrap()).poll(cx));
        self.shutdown = None;
        self.result = Some(result.clone());
        Poll::Ready(result.map_err(IoError::Failed))
    }
    pub(crate) fn shutdown_reason(&mut self, mut error: Error) {
        error.kind = capnp::ErrorKind::Disconnected;
        self.state.borrow_mut().failure = Some(error);
    }
    pub(crate) fn publication_queue<'p, 's>(
        &mut self,
        messages: &messages::Messages<'p, '_, '_>,
        maximum: usize,
    ) -> Result<messages::Queue<'p, 's, Bundle>, messages::Error> {
        self.closing.set_maximum(maximum)?;
        messages::Queue::hosted(
            messages,
            Box::new(PublicationSink {
                state: self.state.clone(),
                closing: self.closing.clone(),
            }),
        )
    }
}
struct PublicationSink {
    state: Rc<RefCell<OutputState>>,
    closing: Rc<messages::OutputStatus>,
}
impl messages::PublicationSink<Bundle> for PublicationSink {
    fn status(&self) -> &Rc<messages::OutputStatus> {
        &self.closing
    }
    fn reserve(&mut self) -> Result<(), messages::Error> {
        let mut state = self.state.borrow_mut();
        state.output.reserve().map_err(|error| {
            self.closing.fail();
            state.failure = Some(error);
            messages::Error::QueueFailed
        })
    }
    fn cancel_reservation(&mut self) {
        self.state.borrow_mut().output.cancel_reservation();
    }
    fn publish_reserved(&mut self, frame: messages::Publication, mut resources: Bundle) {
        let frame = match frame {
            messages::Publication::Bytes(bytes) => crate::OutgoingFrame::from_shared(bytes),
            messages::Publication::Parts(parts) => crate::OutgoingFrame::from_parts(parts),
        };
        let mut frame = match frame {
            Ok(frame) => frame,
            Err(error) => {
                let mut state = self.state.borrow_mut();
                state.output.cancel_reservation();
                self.closing.fail();
                state.failure = Some(error);
                return;
            }
        };
        frame.credit = resources.credit.take();
        #[cfg(unix)]
        if let Some(fds) = resources.fds.take() {
            frame.set_fds(fds.iter().cloned().collect());
        }
        let mut state = self.state.borrow_mut();
        self.closing.published();
        if let Err(error) = state.output.publish_reserved(frame) {
            self.closing.fail();
            state.failure = Some(error);
        }
    }
}
impl Drop for Writer {
    fn drop(&mut self) {
        if !self.closing.close() {
            let mut state = self.state.borrow_mut();
            let error = state
                .failure
                .clone()
                .unwrap_or_else(|| Error::disconnected("portable transport IO abandoned".into()));
            drop(state.output.shutdown(Err(error)));
        }
    }
}
impl io::Flush for Writer {
    type Error = Error;
    fn poll_flush(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), IoError<Error>>> {
        if self.closing.pending() == 0 && !self.closing.failed() {
            return Poll::Ready(Ok(()));
        }
        let mut state = self.state.borrow_mut();
        if let Some(error) = &state.failure {
            return Poll::Ready(Err(IoError::Failed(error.clone())));
        }
        if self.closing.pending() != 0 {
            if let Err(error) = ready!(state.output.poll_flush(cx)) {
                self.closing.fail();
                state.failure = Some(error.clone());
                return Poll::Ready(Err(IoError::Failed(error)));
            }
            self.closing.flushed();
        }
        Poll::Ready(Ok(()))
    }
}
impl io::Close for Writer {
    fn poll_close(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), IoError<Error>>> {
        if let Some(result) = &self.result {
            return Poll::Ready(result.clone().map_err(IoError::Failed));
        }
        ready!(io::Flush::poll_flush(self, cx))?;
        self.poll_shutdown(cx, Ok(()))
    }
    fn poll_abort(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), IoError<Error>>> {
        let error = self
            .state
            .borrow()
            .failure
            .clone()
            .unwrap_or_else(|| Error::disconnected("portable output failed".into()));
        self.poll_shutdown(cx, Err(error))
    }
}

#[derive(Default)]
pub struct Bundle {
    #[cfg(unix)]
    fds: Option<Box<capntproto_rpc_wire::attachments::Sender<Rc<std::os::fd::OwnedFd>, 4>>>,
    credit: Option<Rc<crate::admission::Permit>>,
}

impl io::WriteResources<Bundle> for Writer {
    fn can_write_while_flushing(&self) -> bool {
        // OutgoingFrame owns the bytes, resources and credit. poll_capacity
        // bounds admission; poll_flush remains the physical completion fence.
        true
    }
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
impl Writer {
    fn poll_capacity(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), IoError<Error>>> {
        if self.closing.is_closed() {
            return Poll::Ready(Err(IoError::Failed(Error::disconnected(
                "transport closing".into(),
            ))));
        }
        if let Some(error) = &self.state.borrow().failure {
            return Poll::Ready(Err(IoError::Failed(error.clone())));
        }
        let full = self.closing.full();
        if full {
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
        if let Some(fds) = resources.and_then(|resources| resources.fds.as_ref()) {
            frame.set_fds(fds.iter().cloned().collect());
        }
        let mut state = self.state.borrow_mut();
        state.output.send(frame).map_err(IoError::Failed)?;
        self.closing.published();
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
