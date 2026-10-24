//! Transport resources share the byte frame's admission and FIFO lifetime.
use super::*;
use capntproto_rpc_engine::imports::Imports;
use capntproto_rpc_wire::{attachments::ImportReceiver, clients::Route};
pub mod vat;

#[cfg(unix)]
use capntproto_rpc_wire::{attachments::Sender, clients::Endpoint, Capacity};

/// Reader of exactly one framed message at a time. Reads must never prefetch
/// resources from another frame. Keep all resources through admission retries;
/// attach consumes each index at most once. finish_frame drops unused resources
/// after admission, EOF, failure or cancellation, before any following frame.
/// A discarded run future requires dropping this input along with its pool.
pub trait Input<I>: io::Read + ImportReceiver<I> {
    /// Optionally transfer a complete immutable allocation into the bounded
    /// message pool. Rejection leaves bytes and attachments unconsumed. Success
    /// retains attachments until finish_frame, like take_buffered_frame.
    fn take_shared_frame<'p>(
        &mut self,
        _: &mut messages::Messages<'p, '_, '_>,
        _: capnp::message::ReaderOptions,
    ) -> Result<Option<Frame<'p>>, capntproto_rpc_wire::owned::Error> {
        Ok(None)
    }
    /// A complete next frame is buffered after finishing the previous one.
    /// Inspect only; do not read bytes or consume ancillary resources here.
    /// Validated framing metadata may be retained for the next receive.
    fn has_buffered_next_frame(&mut self) -> bool {
        false
    }
    /// Borrow an untouched complete frame after poll_frame_size, without IO.
    /// A successful direct admission calls consume_buffered_frame; rejection
    /// leaves this frame and its ancillary resources available for normal input.
    fn buffered_frame(&self) -> Option<&[u8]> {
        None
    }
    /// Consume the frame exposed above and release its unused resources.
    /// Implementations exposing a frame must override this method.
    fn consume_buffered_frame(&mut self) {
        self.finish_frame();
    }

    /// Transfer an already buffered complete frame after `poll_frame_size`.
    /// Implementations must enforce the supplied reader options, perform no IO,
    /// and retain attachments until `finish_frame`, including admission retries.
    /// None uses the ordinary fragmented byte reader and its caller-owned task.
    fn take_buffered_frame<'p>(
        &mut self,
        _: &mut messages::Messages<'p, '_, '_>,
        _: capnp::message::ReaderOptions,
    ) -> Result<Option<Frame<'p>>, capntproto_rpc_wire::owned::Error> {
        Ok(None)
    }

    /// A message transport may first receive and validate one complete frame,
    /// then report its framed byte length before pool reservation. None selects
    /// the configured maximum for ordinary byte streams. Retain that message
    /// and its resources until finish_frame; do not prefetch the next frame.
    /// The coordinator still enforces its maximum and validates all read bytes.
    fn poll_frame_size(&mut self, _: &mut Context<'_>) -> Poll<Result<Option<usize>, Self::Error>> {
        Poll::Ready(Ok(None))
    }
    fn finish_frame(&mut self);
}

/// Maps exact capability owners to one outgoing frame's resource bundle.
/// Returning 255 explicitly omits an attachment. Metadata is synchronized before
/// exposing an imported hook to application code, including late import aliases.
pub trait Output<I> {
    /// Stable metadata subscription for an imported route. Every later change
    /// must advance this source. None declares that this import has no metadata.
    fn metadata_source(&self, owner: &I) -> Option<Rc<MetadataSource>>;
    type Bundle: Default;
    fn bundle(&self) -> Self::Bundle;
    /// Retain Call credit through frame acceptance and transport completion.
    /// An adapter must explicitly support this before enabling outgoing limits.
    fn retain_call(
        &self,
        _: &mut Self::Bundle,
        _: Rc<capntproto_rpc_engine::flow_control::outgoing::OwnedPermit>,
    ) -> capnp::Result<()> {
        Err(Error::unimplemented(
            "output cannot retain call admission credit".into(),
        ))
    }
    fn attach<'id, P: Copy, const H: usize>(
        &self,
        owner: &Route<'id, P, Rc<dyn ClientHook>, H>,
        imports: &Imports<'id, '_, I>,
        bundle: &mut Self::Bundle,
    ) -> u8;
}

/// Ordinary TCP/QUIC streams do not carry ancillary resources.
pub struct NoResources;
impl<I> Output<I> for NoResources {
    type Bundle = ();
    fn bundle(&self) {}
    fn attach<'id, P: Copy, const H: usize>(
        &self,
        _: &Route<'id, P, Rc<dyn ClientHook>, H>,
        _: &Imports<'id, '_, I>,
        _: &mut (),
    ) -> u8 {
        u8::MAX
    }
    fn metadata_source(&self, _: &I) -> Option<Rc<MetadataSource>> {
        None
    }
}

pub(super) struct Bytes<'a, R, F> {
    pub reader: &'a mut R,
    pub make: F,
}
impl<R: io::Read, F> io::Read for Bytes<'_, R, F> {
    type Error = R::Error;
    fn poll_read(
        &mut self,
        cx: &mut Context<'_>,
        bytes: &mut [u8],
    ) -> Poll<Result<usize, io::IoError<Self::Error>>> {
        self.reader.poll_read(cx, bytes)
    }
}
impl<I, R, F: FnMut(ImportId) -> I> ImportReceiver<I> for Bytes<'_, R, F> {
    fn make_import(&mut self, id: ImportId) -> I {
        (self.make)(id)
    }
}
impl<I, R: io::Read, F: FnMut(ImportId) -> I> Input<I> for Bytes<'_, R, F> {
    fn finish_frame(&mut self) {}
}
pub(super) struct Writer<'a, W>(pub &'a mut W);
impl<W: io::Flush> io::Flush for Writer<'_, W> {
    type Error = W::Error;
    fn poll_flush(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), io::IoError<Self::Error>>> {
        self.0.poll_flush(cx)
    }
}
impl<W: io::Close> io::Close for Writer<'_, W> {
    fn poll_close(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), io::IoError<Self::Error>>> {
        self.0.poll_close(cx)
    }
}
impl<W: io::Write> io::WriteResources<()> for Writer<'_, W> {
    fn poll_write_resources(
        &mut self,
        cx: &mut Context<'_>,
        bytes: &[u8],
        _: Option<&()>,
    ) -> Poll<Result<usize, io::IoError<Self::Error>>> {
        self.0.poll_write(cx, bytes)
    }
}

/// Unix descriptor policy; the original hook controls descriptor visibility.
/// Inline capacity stays bounded even when alloc is enabled. Growing capacity
/// must be explicitly selected and is additionally capped by the wire sentinel.
#[cfg(unix)]
pub struct FileDescriptors<const N: usize> {
    pub capacity: Capacity,
}
#[cfg(unix)]
impl<const N: usize> Default for FileDescriptors<N> {
    fn default() -> Self {
        Self {
            capacity: Capacity::Inline,
        }
    }
}
#[cfg(unix)]
impl<T, const N: usize> Output<resources::Import<T>> for FileDescriptors<N> {
    type Bundle = Sender<Rc<std::os::fd::OwnedFd>, N>;
    fn bundle(&self) -> Self::Bundle {
        Sender::new(self.capacity)
    }
    fn attach<'id, P: Copy, const H: usize>(
        &self,
        owner: &Route<'id, P, Rc<dyn ClientHook>, H>,
        imports: &Imports<'id, '_, resources::Import<T>>,
        bundle: &mut Self::Bundle,
    ) -> u8 {
        bundle.push(match owner {
            Route::Capability(Endpoint::Imported(remote)) => {
                imports.get(remote.reference()).metadata().fd
            }
            Route::Capability(Endpoint::Local(hook)) | Route::Vine(hook) => hook.get_fd(),
            _ => None,
        })
    }
    fn metadata_source(&self, owner: &resources::Import<T>) -> Option<Rc<MetadataSource>> {
        Some(owner.metadata_source())
    }
}
