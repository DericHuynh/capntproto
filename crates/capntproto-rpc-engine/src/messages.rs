//! Scoped message buffers and sends, with caller-backed allocation-free storage
//! and optional bounded, growing storage. IO is always caller-supplied.
//!
//! Writers are exclusive; sealing exposes only an immutable used prefix. Read
//! leases and offered/accepted sends independently retain that prefix. A buffer
//! is reusable only after both counts reach zero. Reservation clears old bytes.
//! The caller chooses buffer alignment, sizes, send capacity and polling order.
//!
//! Scoped tokens cannot escape or cross pools. Dropped/forgotten writer, reader
//! and offer tokens retain their reservations until explicit cancellation or
//! terminal scope teardown. A dropped send window preserves partial progress.
//! Send completion means the adapter consumed the bytes, not peer receipt or
//! RPC completion. Quiesce IO before ending a nonempty scope.
//!
//! ```
//! use capntproto_rpc_engine::messages::{with_messages, Buffer, SendSlot};
//! let mut bytes = [0; 8];
//! let mut buffers = [Buffer::new(&mut bytes)];
//! let mut sends = [SendSlot::EMPTY];
//! with_messages(&mut buffers, &mut sends, |messages| {
//!     let writer = messages.reserve(8).unwrap();
//!     messages.write(&writer)[..3].copy_from_slice(b"rpc");
//!     let lease = messages.seal(writer, 3).unwrap();
//!     let offer = messages.offer(&lease).unwrap();
//!     let key = messages.accept(offer);
//!     messages.release(lease); // The send still owns these bytes.
//!     let window = messages.send_window(key).unwrap();
//!     assert_eq!(window.bytes(), b"rpc");
//!     // Commit the count after the IO adapter consumes this exact prefix.
//!     assert_eq!(window.advance(3), Ok(true));
//!     let writer = messages.reserve(8).unwrap();
//!     messages.discard(writer);
//! }).unwrap();
//! ```
use core::{
    cell::{Cell, Ref, RefCell, RefMut},
    fmt,
    marker::PhantomData,
};
mod backing;
mod policy;
#[cfg(feature = "alloc")]
pub use backing::SharedBytes;
use backing::{Buffers, Bytes};
use policy::State;
#[cfg(capntproto_verus)]
use vstd::prelude::*;
#[cfg(feature = "alloc")]
mod growing;
#[cfg(feature = "alloc")]
pub use growing::{with_growing_messages, with_growing_messages_async, GrowingBuffers};
mod queue;
pub use queue::{Queue, QueueEntry, QueueSlot, QueueWindow};

type Brand<'id> = PhantomData<fn(&'id ()) -> &'id ()>;

#[cfg_attr(capntproto_verus, verus_verify)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    NonemptyStorage,
    NoBuffer,
    NoSendSlot,
    GenerationExhausted,
    InvalidLength,
    ReferencesExhausted,
    StaleSend,
    InvalidWriteCount,
    WriteZero,
    QueueFailed,
    QueueStillLive,
    BorrowedBuffer,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NonemptyStorage => "message storage is not empty",
            Self::NoBuffer => "no reusable message buffer of the requested capacity",
            Self::NoSendSlot => "send storage is full",
            Self::GenerationExhausted => "message or send generation exhausted",
            Self::InvalidLength => "sealed length exceeds buffer capacity",
            Self::ReferencesExhausted => "message reference count exhausted",
            Self::StaleSend => "send completion is stale or not accepted",
            Self::InvalidWriteCount => "write completion exceeds remaining bytes",
            Self::WriteZero => "write made no progress on a nonempty message",
            Self::QueueFailed => "ordered output failed; terminate the connection",
            Self::QueueStillLive => "terminate output before discarding accepted sends",
            Self::BorrowedBuffer => "a forgotten mutable buffer view remains reserved",
        })
    }
}
impl core::error::Error for Error {}

/// Caller-owned byte storage; neither alignment nor Cap'n Proto validity is
/// implied by this pool. The framing/decoder layer validates those properties.
pub struct Buffer<'bytes> {
    bytes: RefCell<Bytes<'bytes>>,
    capacity: Cell<usize>,
    generation: Cell<u64>,
    state: Cell<State>,
}
impl<'bytes> Buffer<'bytes> {
    /// Usable bytes, excluding any alignment padding in allocating storage.
    pub fn capacity(&self) -> usize {
        self.capacity.get()
    }
    pub const fn new(bytes: &'bytes mut [u8]) -> Self {
        Self {
            capacity: Cell::new(bytes.len()),
            bytes: RefCell::new(Bytes::Borrowed(bytes)),
            generation: Cell::new(0),
            state: Cell::new(State::Free),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BufferKey {
    index: usize,
    generation: u64,
}

#[derive(Debug)]
#[must_use = "seal or discard the writer, or terminate the message scope"]
pub struct Writer<'id> {
    key: BufferKey,
    length: usize,
    brand: Brand<'id>,
}

/// An exclusive buffer reservation and its mutable view travel together. This
/// can be moved into a caller-pinned async IO/server task without borrowing the
/// entire message pool. No slice may outlive this owner, and consuming it drops
/// the view before exposing the writer needed for sealing or discarding.
#[must_use = "recover and seal/discard its writer, or terminate the pool scope"]
pub struct Writing<'id, 'storage> {
    writer: Writer<'id>,
    view: RefMut<'storage, [u8]>,
}
impl<'id> Writing<'id, '_> {
    pub fn bytes(&mut self) -> &mut [u8] {
        &mut self.view
    }
    pub fn into_writer(self) -> Writer<'id> {
        let Self { writer, view } = self;
        drop(view);
        writer
    }
}

#[derive(Debug)]
#[must_use = "release this message lease or terminate the scope"]
pub struct Lease<'id> {
    key: BufferKey,
    brand: Brand<'id>,
}

/// Rejected sealing returns the writer intact so the caller can retry/discard.
#[derive(Debug)]
pub struct SealError<'id> {
    pub error: Error,
    pub writer: Writer<'id>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Offered,
    Accepted,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Send {
    buffer: BufferKey,
    position: usize,
    phase: Phase,
}

pub struct SendSlot {
    generation: u64,
    send: Option<Send>,
}
impl SendSlot {
    pub const EMPTY: Self = Self {
        generation: 0,
        send: None,
    };
}

/// A completion identity becomes public only after queue acceptance. Copies
/// carry no extra ownership; duplicate or late completions are checked errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SendKey<'id> {
    index: usize,
    generation: u64,
    brand: Brand<'id>,
}

#[derive(Debug)]
#[must_use = "accept or reject this offered send, or terminate the scope"]
pub struct Offer<'id> {
    key: SendKey<'id>,
}

/// Rejected ownership transfer preserves the caller's original reader lease.
#[derive(Debug)]
pub struct OfferError<'id> {
    pub error: Error,
    pub lease: Lease<'id>,
}

pub struct Messages<'id, 'storage, 'bytes> {
    buffers: Buffers<'storage, 'bytes>,
    sends: crate::storage::Storage<'storage, SendSlot>,
    brand: Brand<'id>,
}

/// Introduce a fresh invariant pool domain without globals or atomics. Ending
/// the scope reclaims records, preserving generations; no cancellation IO is
/// performed. Actual IO must have stopped borrowing the bytes before teardown.
pub fn with_messages<'storage, 'bytes, R>(
    buffers: &'storage mut [Buffer<'bytes>],
    sends: &'storage mut [SendSlot],
    f: impl for<'id> FnOnce(&mut Messages<'id, 'storage, 'bytes>) -> R,
) -> Result<R, Error> {
    Ok(f(&mut new_scope(buffers, sends)?))
}
/// Keep caller-backed leases and partial sends across await without allocating.
/// IO and scheduling are supplied by the closure/caller. Before dropping this
/// future, quiesce IO and treat a partially written stream as terminal.
pub async fn with_messages_async<'storage, 'bytes, R>(
    buffers: &'storage mut [Buffer<'bytes>],
    sends: &'storage mut [SendSlot],
    f: impl for<'id> core::ops::AsyncFnOnce(&mut Messages<'id, 'storage, 'bytes>) -> R,
) -> Result<R, Error> {
    Ok(f(&mut new_scope(buffers, sends)?).await)
}
fn new_scope<'id, 'storage, 'bytes>(
    buffers: &'storage mut [Buffer<'bytes>],
    sends: &'storage mut [SendSlot],
) -> Result<Messages<'id, 'storage, 'bytes>, Error> {
    if buffers.iter().any(|b| b.state.get() != State::Free)
        || sends.iter().any(|s| s.send.is_some())
    {
        return Err(Error::NonemptyStorage);
    }
    // Exclusive storage access proves no live view exists. Recover borrow flags
    // from deliberately forgotten views in an earlier, terminated scope.
    for buffer in &mut *buffers {
        let bytes = core::mem::take(buffer.bytes.get_mut());
        buffer.bytes = RefCell::new(bytes);
    }
    Ok(Messages {
        buffers: Buffers::Borrowed(buffers),
        sends: crate::storage::Storage::borrowed(sends),
        brand: PhantomData,
    })
}

impl<'id, 'storage, 'bytes> Messages<'id, 'storage, 'bytes> {
    /// Reserve the first reusable buffer which fits, clearing its whole capacity
    /// before exposing it. Exhausted generations are never reused.
    pub fn reserve(&mut self, minimum: usize) -> Result<Writer<'id>, Error> {
        self.reserve_length(minimum, false)
    }
    /// Reserve and clear exactly `length` bytes, even when reusing a larger
    /// buffer. All writer views and sealing are limited to that prefix. Bytes
    /// beyond it remain inaccessible until a later reservation clears them.
    pub fn reserve_exact(&mut self, length: usize) -> Result<Writer<'id>, Error> {
        self.reserve_length(length, true)
    }
    /// Copy a complete message into reusable storage in one exclusive borrow.
    /// The entire exposed prefix is overwritten before sealing; no mutable
    /// view or partially initialized writer can escape this operation.
    pub fn copy_from(&mut self, source: &[u8]) -> Result<Lease<'id>, Error> {
        self.copy_from_parts(&[source], |_| {})
    }

    /// Copy a complete initialized message, then edit it before sealing. No
    /// mutable view escapes the callback and only copied bytes are exposed.
    /// Failure to reserve preserves every live owner. If the callback unwinds,
    /// the unused buffer remains free and its exclusive view is dropped.
    #[inline(always)]
    pub fn copy_from_parts(
        &mut self,
        parts: &[&[u8]],
        edit: impl FnOnce(&mut [u8]),
    ) -> Result<Lease<'id>, Error> {
        let length = parts.iter().try_fold(0usize, |length, part| {
            length.checked_add(part.len()).ok_or(Error::InvalidLength)
        })?;
        let initialize = |bytes: &mut [u8]| {
            let bytes = &mut bytes[..length];
            let mut remaining = &mut bytes[..];
            for part in parts {
                let (next, rest) = remaining.split_at_mut(part.len());
                next.copy_from_slice(part);
                remaining = rest;
            }
            edit(bytes);
        };
        let mut exhausted = false;
        for (index, buffer) in self.buffers.iter().enumerate() {
            if buffer.state.get() != State::Free || buffer.capacity.get() < length {
                continue;
            }
            let Ok(mut bytes) = buffer.bytes.try_borrow_mut() else {
                continue;
            };
            if !bytes.exclusive() {
                continue;
            }
            if buffer.generation.get() == u64::MAX {
                exhausted = true;
                continue;
            }
            initialize(&mut bytes);
            buffer.generation.set(buffer.generation.get() + 1);
            buffer.state.set(State::Sealed {
                length,
                readers: 1,
                sends: 0,
            });
            return Ok(Lease {
                key: BufferKey {
                    index,
                    generation: buffer.generation.get(),
                },
                brand: PhantomData,
            });
        }
        let index = self.buffers.append(length).ok_or(if exhausted {
            Error::GenerationExhausted
        } else {
            Error::NoBuffer
        })?;
        let buffer = &self.buffers[index];
        initialize(&mut buffer.bytes.borrow_mut());
        buffer.generation.set(buffer.generation.get() + 1);
        buffer.state.set(State::Sealed {
            length,
            readers: 1,
            sends: 0,
        });
        Ok(Lease {
            key: BufferKey {
                index,
                generation: buffer.generation.get(),
            },
            brand: PhantomData,
        })
    }
    fn reserve_length(&mut self, minimum: usize, exact: bool) -> Result<Writer<'id>, Error> {
        let mut exhausted = false;
        for (index, buffer) in self.buffers.iter().enumerate() {
            if buffer.state.get() != State::Free || buffer.capacity.get() < minimum {
                continue;
            }
            let Ok(mut bytes) = buffer.bytes.try_borrow_mut() else {
                continue;
            };
            if !bytes.exclusive() {
                continue;
            }
            if buffer.generation.get() == u64::MAX {
                exhausted = true;
                continue;
            }
            // Keep the checked exclusive view through initialization. Looking
            // this buffer up and borrowing it a second time adds no guarantee.
            let length = if exact {
                minimum
            } else {
                buffer.capacity.get()
            };
            bytes[..length].fill(0);
            buffer.generation.set(buffer.generation.get() + 1);
            buffer.state.set(State::Writing);
            return Ok(Writer {
                length,
                key: BufferKey {
                    index,
                    generation: buffer.generation.get(),
                },
                brand: PhantomData,
            });
        }
        let index = self.buffers.append(minimum).ok_or(if exhausted {
            Error::GenerationExhausted
        } else {
            Error::NoBuffer
        })?;
        let buffer = &self.buffers[index];
        buffer.generation.set(buffer.generation.get() + 1);
        let length = if exact {
            minimum
        } else {
            buffer.capacity.get()
        };
        buffer.bytes.borrow_mut()[..length].fill(0);
        buffer.state.set(State::Writing);
        Ok(Writer {
            length,
            key: BufferKey {
                index,
                generation: buffer.generation.get(),
            },
            brand: PhantomData,
        })
    }
    pub fn write(&mut self, writer: &Writer<'id>) -> RefMut<'_, [u8]> {
        self.try_write(writer)
            .expect("writer has no forgotten mutable view")
    }
    /// Fallible access also handles a deliberately forgotten previous view.
    /// Such a view reserves the bytes until terminal scope teardown.
    pub fn try_write(&mut self, writer: &Writer<'id>) -> Result<RefMut<'_, [u8]>, Error> {
        self.buffers[writer.key.index]
            .bytes
            .try_borrow_mut()
            .map(|view| RefMut::map(view, |bytes| &mut bytes[..writer.length]))
            .map_err(|_| Error::BorrowedBuffer)
    }
    /// Transfer a writer into an independently owned view. Rejection preserves
    /// the writer; a forgotten old view remains a checked capacity failure.
    pub fn write_owned(
        &self,
        writer: Writer<'id>,
    ) -> Result<Writing<'id, 'storage>, SealError<'id>> {
        let buffer = self.buffers.at(writer.key.index);
        let length = writer.length;
        match buffer.bytes.try_borrow_mut() {
            Ok(view) => Ok(Writing {
                writer,
                view: RefMut::map(view, |bytes| &mut bytes[..length]),
            }),
            Err(_) => Err(SealError {
                error: Error::BorrowedBuffer,
                writer,
            }),
        }
    }
    /// Borrow only this reservation while other message buffers progress. The
    /// mutable writer borrow prevents sealing or discarding it before the view
    /// is dropped. This permits a generated result builder to survive caller-
    /// polled suspension without borrowing the entire connection's output pool.
    pub fn write_borrowed<'view>(
        &self,
        writer: &'view mut Writer<'id>,
    ) -> Result<RefMut<'view, [u8]>, Error>
    where
        'storage: 'view,
    {
        let buffer = self.buffers.at(writer.key.index);
        debug_assert_eq!(buffer.generation.get(), writer.key.generation);
        debug_assert_eq!(buffer.state.get(), State::Writing);
        buffer
            .bytes
            .try_borrow_mut()
            .map(|view| RefMut::map(view, |bytes| &mut bytes[..writer.length]))
            .map_err(|_| Error::BorrowedBuffer)
    }
    pub fn discard(&mut self, writer: Writer<'id>) {
        self.buffers[writer.key.index].state.set(State::Free);
    }
    pub fn seal(
        &mut self,
        writer: Writer<'id>,
        length: usize,
    ) -> Result<Lease<'id>, SealError<'id>> {
        let buffer = &self.buffers[writer.key.index];
        if length > writer.length {
            return Err(SealError {
                error: Error::InvalidLength,
                writer,
            });
        }
        if buffer.bytes.try_borrow().is_err() {
            return Err(SealError {
                error: Error::BorrowedBuffer,
                writer,
            });
        }
        buffer.state.set(State::Sealed {
            length,
            readers: 1,
            sends: 0,
        });
        Ok(Lease {
            key: writer.key,
            brand: PhantomData,
        })
    }
    /// Retain a zero-copy view without borrowing the connection's entire pool.
    /// The view borrows this lease: release it only after the view is dropped.
    /// Other buffers, reference counts and sends remain available meanwhile.
    #[inline(always)]
    pub fn read<'view>(&self, lease: &'view Lease<'id>) -> Ref<'view, [u8]>
    where
        'storage: 'view,
    {
        self.bytes(lease.key)
    }
    #[inline(always)]
    fn bytes(&self, key: BufferKey) -> Ref<'storage, [u8]> {
        let buffer = self.buffers.at(key.index);
        debug_assert_eq!(buffer.generation.get(), key.generation);
        let State::Sealed { length, .. } = buffer.state.get() else {
            unreachable!("live message owner")
        };
        Ref::map(buffer.bytes.borrow(), |bytes| &bytes[..length])
    }
    pub fn retain(&mut self, lease: &Lease<'id>) -> Result<Lease<'id>, Error> {
        let state = &self.buffers[lease.key.index].state;
        state.set(policy::retain(state.get(), false)?);
        Ok(Lease {
            key: lease.key,
            brand: PhantomData,
        })
    }
    /// Retain the sealed prefix independently of this scope when the provider
    /// owns its allocation. Caller-borrowed buffers return None. A shared owner
    /// pins this buffer against reuse and counts toward the provider's limits.
    #[cfg(feature = "alloc")]
    #[inline(always)]
    pub fn share(&self, lease: &Lease<'id>) -> Option<SharedBytes> {
        let buffer = &self.buffers[lease.key.index];
        let State::Sealed { length, .. } = buffer.state.get() else {
            unreachable!("live lease")
        };
        buffer.bytes.borrow().share(0..length)
    }
    #[inline(always)]
    pub fn release(&mut self, lease: Lease<'id>) {
        let state = &self.buffers[lease.key.index].state;
        let next = policy::release(state.get(), false);
        state.set(if policy::reclaimable(next) {
            State::Free
        } else {
            next
        });
    }

    /// Reserve send storage and its message reference before queue acceptance.
    /// Failure leaves both capacities/counters unchanged, preserving the lease.
    pub fn offer(&mut self, lease: &Lease<'id>) -> Result<Offer<'id>, Error> {
        self.offer_inner(lease.key, false)
    }
    /// Transfer a reader lease into an offered send in one transaction. No
    /// buffer becomes free between the two owners. On rejection all counters
    /// and generations are unchanged and the original lease is returned.
    pub fn offer_owned(&mut self, lease: Lease<'id>) -> Result<Offer<'id>, OfferError<'id>> {
        self.offer_inner(lease.key, true)
            .map_err(|error| OfferError { error, lease })
    }
    #[inline(always)]
    fn offer_inner(&mut self, buffer: BufferKey, consume: bool) -> Result<Offer<'id>, Error> {
        let state = &self.buffers[buffer.index].state;
        let next = policy::retain(state.get(), true)?;
        let next = if consume {
            policy::release(next, false)
        } else {
            next
        };
        let index = self
            .sends
            .iter()
            .position(|s| s.send.is_none() && s.generation != u64::MAX)
            .or_else(|| {
                let index = self.sends.len();
                self.sends.grow(1, || SendSlot::EMPTY).then_some(index)
            })
            .ok_or_else(|| {
                if self.sends.iter().any(|s| s.send.is_none()) {
                    Error::GenerationExhausted
                } else {
                    Error::NoSendSlot
                }
            })?;
        let slot = &mut self.sends[index];
        slot.generation += 1;
        slot.send = Some(Send {
            buffer,
            position: 0,
            phase: Phase::Offered,
        });
        state.set(next);
        Ok(Offer {
            key: SendKey {
                index,
                generation: slot.generation,
                brand: PhantomData,
            },
        })
    }
    /// Output rejected this reservation; it cannot have borrowed message bytes
    /// through an accepted send. Drop only the offered send's reference.
    pub fn reject(&mut self, offer: Offer<'id>) {
        self.finish_send(offer.key.index);
    }
    /// Queue acceptance preserves the buffer; it does not report transmitted bytes.
    pub fn accept(&mut self, offer: Offer<'id>) -> SendKey<'id> {
        self.sends[offer.key.index]
            .send
            .as_mut()
            .expect("live offer")
            .phase = Phase::Accepted;
        offer.key
    }
    fn accepted(&self, key: SendKey<'id>) -> Result<Send, Error> {
        let slot = self.sends.get(key.index).ok_or(Error::StaleSend)?;
        if slot.generation != key.generation {
            return Err(Error::StaleSend);
        }
        slot.send
            .filter(|send| send.phase == Phase::Accepted)
            .ok_or(Error::StaleSend)
    }
    /// Borrow the unsent suffix for one synchronous/polled IO attempt. On
    /// Pending, drop the window; completed writes must commit their byte count.
    pub fn send_window(
        &mut self,
        key: SendKey<'id>,
    ) -> Result<SendWindow<'_, 'id, 'storage, 'bytes>, Error> {
        let slot = self.sends.get_mut(key.index).ok_or(Error::StaleSend)?;
        if slot.generation != key.generation {
            return Err(Error::StaleSend);
        }
        let send = slot
            .send
            .filter(|send| send.phase == Phase::Accepted)
            .ok_or(Error::StaleSend)?;
        let buffer = self.buffers.at(send.buffer.index);
        let State::Sealed { length, .. } = buffer.state.get() else {
            unreachable!("accepted send owns sealed bytes")
        };
        let bytes = Ref::map(buffer.bytes.borrow(), |bytes| &bytes[send.position..length]);
        Ok(SendWindow {
            send: &mut slot.send,
            buffer,
            bytes,
            brand: PhantomData,
        })
    }
    /// The adapter has stopped using this send's bytes after cancellation/error.
    /// A partial framed stream failure also requires caller-managed connection
    /// termination; releasing storage cannot undo already transmitted bytes.
    pub fn fail_send(&mut self, key: SendKey<'id>) -> Result<(), Error> {
        self.accepted(key)?;
        self.finish_send(key.index);
        Ok(())
    }
    fn finish_send(&mut self, index: usize) {
        let send = self.sends[index].send.take().expect("live send");
        let state = &self.buffers[send.buffer.index].state;
        let next = policy::release(state.get(), true);
        state.set(if policy::reclaimable(next) {
            State::Free
        } else {
            next
        });
    }
}

impl Drop for Messages<'_, '_, '_> {
    fn drop(&mut self) {
        for slot in &mut *self.sends {
            slot.send = None;
        }
        for buffer in self.buffers.iter() {
            buffer.state.set(State::Free);
        }
    }
}

/// An exclusive completion window. The bytes remain immutable and borrowed
/// until the caller commits or drops the window; neither action allocates.
pub struct SendWindow<'a, 'id, 'storage, 'bytes> {
    // Validate the generation once when acquiring these exclusive owners. The
    // slot cannot be replaced or reused until this window ends.
    send: &'a mut Option<Send>,
    buffer: &'storage Buffer<'bytes>,
    bytes: Ref<'storage, [u8]>,
    brand: Brand<'id>,
}
impl SendWindow<'_, '_, '_, '_> {
    /// Whether no bytes have been accepted for this exact send. Resource-aware
    /// queues consult the authoritative cursor rather than a second progress bit.
    pub fn is_start(&self) -> bool {
        self.send.expect("live window").position == 0
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Retain exactly the unsent suffix for an owning transport adapter.
    #[cfg(feature = "alloc")]
    pub fn share(&self) -> Option<SharedBytes> {
        let send = self.send.expect("live window");
        self.buffer
            .bytes
            .borrow()
            .share(send.position..send.position + self.bytes.len())
    }
    /// True means all bytes were consumed by IO and this send reference was
    /// released. Zero on an empty message completes without an IO call; zero
    /// on a nonempty suffix is an error. Invalid counts preserve all progress.
    pub fn advance(self, count: usize) -> Result<bool, Error> {
        let send = self.send.expect("live window");
        let remaining = self.bytes.len();
        let (next, complete) = policy::advance(send.position, remaining, count)?;
        drop(self.bytes);
        if complete {
            self.send.take();
            let state = &self.buffer.state;
            let next = policy::release(state.get(), true);
            state.set(if policy::reclaimable(next) {
                State::Free
            } else {
                next
            });
            Ok(true)
        } else {
            self.send.as_mut().unwrap().position = next;
            Ok(false)
        }
    }
}

#[cfg(kani)]
mod proofs;
#[cfg(test)]
mod tests;
