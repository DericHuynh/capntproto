#![forbid(unsafe_code)]
#![cfg_attr(not(feature = "std"), no_std)]
// Copyright (c) 2013-2017 Sandstorm Development Group, Inc. and contributors
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
// THE SOFTWARE.

//! An implementation of the [Cap'n Proto remote procedure call](https://capnproto.org/rpc.html)
//! protocol. Includes all [Level 1](https://capnproto.org/rpc.html#protocol-features) features.
//!
//! With default features disabled, this crate is `no_std` and requires no allocator.
//! Use [`bounded`] for caller-backed RPC dispatch and [`engine`] for its resource
//! and polling contracts. `alloc` adds shared reply ownership; `std` (the default)
//! enables the existing owned clients, server hooks and transport adapters.
//! These profiles do not make a caller-provided service or IO implementation allocation-free.
//!
//! # Example
//!
//! ```capnp
//! # Cap'n Proto schema
//! interface Foo {
//!     identity @0 (x: UInt32) -> (y: UInt32);
//! }
//! ```
//!
//! ```ignore
//! // Rust server defining an implementation of Foo.
//! struct FooImpl;
//! impl foo::Server for FooImpl {
//!     fn identity(
//!         self: Rc<Self>,
//!         context: foo::IdentityContext<'_>,
//!     ) -> capnp::capability::Promise<(), capnp::Error> {
//!         context.complete(|params, mut results| {
//!             results.set_y(params.get_x());
//!             Ok(())
//!         })
//!     }
//! }
//! ```
//!
//! ```ignore
//! // Rust client calling a remote implementation of Foo.
//! let mut request = foo_client.identity_request();
//! request.get().set_x(123);
//! let promise = request.send().promise.and_then(|response| {
//!     println!("results = {}", response.get()?.get_y());
//!     Ok(())
//! });
//! ```
//!
//! For a more complete example, see <https://github.com/capnproto/capnproto-rust/tree/master/capnp-rpc/examples/calculator>

/// Portable protocol transitions and caller-owned resources. The optional
/// `alloc` profile also enables shared completion owners without requiring std.
pub use capntproto_rpc_engine as engine;
/// Allocation-free wire ownership, generated call bindings and caller-polled drivers.
/// Available in every profile; all storage, IO and execution remain caller supplied.
pub use capntproto_rpc_wire as bounded;

#[cfg(feature = "std")]
use capnp::capability::Promise;
#[cfg(feature = "std")]
use capnp::private::capability::ClientHook;
#[cfg(feature = "std")]
use capnp::Error;
#[cfg(feature = "std")]
use futures::{Future, FutureExt};
#[cfg(feature = "std")]
use std::cell::RefCell;
#[cfg(feature = "std")]
use std::pin::Pin;
#[cfg(feature = "std")]
use std::rc::{Rc, Weak};
#[cfg(feature = "std")]
use std::task::{Context, Poll};

#[cfg(feature = "std")]
pub use crate::rpc::join::Joiner;
#[cfg(feature = "std")]
pub use crate::system::Disconnector;
/// Task-owned hosted runtime over the portable engine. This opt-in replacement
/// integrates authenticated introductions, independent-path Join, answer handoff
/// and third-party tail transfer. Live flow limits and diagnostics are supported.
/// It remains opt-in pending performance qualification and remaining Join parity.
#[cfg(feature = "std")]
pub mod portable;
#[cfg(feature = "std")]
use crate::task_set::TaskSet;

#[cfg(feature = "std")]
pub use crate::reconnect::{auto_reconnect, lazy_auto_reconnect, SetTarget};
#[cfg(feature = "std")]
mod pipeline_builder;
#[cfg(feature = "std")]
pub use pipeline_builder::PipelineBuilder;

/// Code generated from
/// [rpc.capnp](https://github.com/capnproto/capnproto/blob/master/c%2B%2B/src/capnp/rpc.capnp).
pub use capntproto_rpc_wire::rpc_capnp;

/// Code generated from
/// [rpc-twoparty.capnp](https://github.com/capnproto/capnproto/blob/master/c%2B%2B/src/capnp/rpc-twoparty.capnp).
pub use capntproto_rpc_wire::rpc_twoparty_capnp;

/// Like [`try!()`], but for functions that return a [`Promise<T, E>`] rather than a [`Result<T, E>`].
///
/// Unwraps a `Result<T, E>`. In the case of an error `Err(e)`, immediately returns from the
/// enclosing function with `Promise::err(e)`.
#[macro_export]
#[cfg(feature = "std")]
macro_rules! pry {
    ($expr:expr) => {
        match $expr {
            ::std::result::Result::Ok(val) => val,
            ::std::result::Result::Err(err) => {
                return ::capnp::capability::Promise::err(::std::convert::From::from(err))
            }
        }
    };
}

#[cfg(feature = "std")]
mod admission;
#[cfg(feature = "std")]
mod attach;
#[cfg(feature = "std")]
pub use admission::{ConnectionSnapshot, RpcDiagnostics};
#[cfg(feature = "std")]
mod bootstrap;
#[cfg(feature = "std")]
pub use bootstrap::Bootstrapper;
#[cfg(feature = "std")]
mod broken;
#[cfg(feature = "std")]
mod fd;
#[cfg(feature = "std")]
mod flow_control;
#[cfg(feature = "std")]
mod local;
#[cfg(feature = "std")]
pub mod membrane;
#[cfg(feature = "std")]
pub mod multiparty;
#[cfg(feature = "std")]
mod queued;
#[cfg(feature = "std")]
mod reconnect;
#[cfg(feature = "std")]
mod revocable;
#[cfg(feature = "std")]
pub mod scoped;
#[cfg(feature = "std")]
pub use revocable::RevocableServer;
#[cfg(feature = "std")]
mod rpc;
#[cfg(feature = "std")]
mod sender_queue;
#[cfg(feature = "std")]
mod split;
#[cfg(feature = "std")]
mod system;
#[cfg(feature = "std")]
mod transport_frame;
#[cfg(feature = "std")]
pub use transport_frame::OutgoingFrame;
#[cfg(feature = "std")]
mod task_set;
#[cfg(feature = "std")]
pub mod third_party;
#[cfg(feature = "std")]
pub mod twoparty;

#[cfg(feature = "std")]
use capnp::message;

/// A message to be sent by a [`VatNetwork`].
#[cfg(feature = "std")]
pub trait OutgoingMessage {
    /// Attach owned descriptor references. Retain these until the queued write
    /// completes, even if the application drops its capability or send promise.
    /// A transport without descriptor support may discard them (the default).
    #[cfg(unix)]
    fn set_fds(&mut self, _fds: Vec<Rc<std::os::fd::OwnedFd>>) {}

    /// Gets the message body, which the caller may fill in any way it wants.
    ///
    /// The standard RPC implementation initializes it as a Message as defined
    /// in `schema/rpc.capnp`.
    fn get_body(&mut self) -> ::capnp::Result<::capnp::any_pointer::Builder<'_>>;

    /// Same as `get_body()`, but returns the corresponding reader type.
    fn get_body_as_reader(&self) -> ::capnp::Result<::capnp::any_pointer::Reader<'_>>;

    /// Initialize a standard Call envelope. Transports may retain its selected
    /// builder until unrestricted `get_body()` access invalidates that selection.
    fn init_call(&mut self) -> capnp::Result<crate::rpc_capnp::call::Builder<'_>> {
        Ok(self
            .get_body()?
            .get_as::<crate::rpc_capnp::message::Builder<'_>>()?
            .init_call())
    }

    /// Borrow an existing standard Call, rejecting a different message kind.
    fn get_call(&mut self) -> capnp::Result<crate::rpc_capnp::call::Builder<'_>> {
        match self
            .get_body()?
            .get_as::<crate::rpc_capnp::message::Builder<'_>>()?
            .which()?
        {
            crate::rpc_capnp::message::Call(call) => call,
            _ => Err(Error::failed("request does not contain Call".into())),
        }
    }

    /// Borrow an existing Return. Unrestricted body access must invalidate any
    /// cached selection, just as it does for Call envelopes.
    fn get_return(&mut self) -> capnp::Result<crate::rpc_capnp::return_::Builder<'_>> {
        match self
            .get_body()?
            .get_as::<crate::rpc_capnp::message::Builder<'_>>()?
            .which()?
        {
            crate::rpc_capnp::message::Return(returned) => returned,
            _ => Err(Error::failed("response does not contain Return".into())),
        }
    }

    /// Set Call metadata without exporting a temporary builder across the
    /// transport boundary. Existing parameters and question ID are preserved.
    fn set_call_header(
        &mut self,
        interface_id: u64,
        method_id: u16,
        no_promise_pipelining: bool,
        allow_third_party_tail_call: bool,
    ) -> capnp::Result<()> {
        let mut call = self.get_call()?;
        call.set_interface_id(interface_id);
        call.set_method_id(method_id);
        call.set_no_promise_pipelining(no_promise_pipelining);
        call.set_allow_third_party_tail_call(allow_third_party_tail_call);
        Ok(())
    }

    /// Sends the message. Returns a promise that resolves once the send has completed.
    /// Dropping the returned promise does *not* cancel the send.
    fn send(
        self: Box<Self>,
    ) -> (
        Promise<(), Error>,
        Rc<message::Builder<message::HeapAllocator>>,
    );

    /// Sends without observing individual write completion. The transport
    /// still owns the message through flush, failure, or writer cancellation.
    /// Write errors must still reach the network's execution driver. The
    /// default preserves compatibility with transports implementing `send()`.
    fn send_detached(self: Box<Self>) -> Rc<message::Builder<message::HeapAllocator>> {
        self.send().1
    }

    /// Publish after protocol state is committed and its table borrows are
    /// released. The transport may immediately poll its existing output driver,
    /// including application-provided IO. Accepted writes retain ordinary FIFO,
    /// lifetime and error semantics; a blocked writer resumes through its waker.
    fn send_detached_and_drive(self: Box<Self>) {
        drop(self.send_detached());
    }

    /// Retains a local lifetime guard through write completion or failure.
    /// `Ok(())` transfers the guard to the transport's queued write; dropping
    /// a send receipt must not release it early. Taking or dropping an unsent
    /// message releases its guard. Unsupported transports return the guard
    /// unchanged so the caller can attach it to a polled completion promise.
    /// An existing guard must never be replaced before its write completes.
    fn retain_until_sent(
        &mut self,
        guard: Rc<dyn std::any::Any>,
    ) -> Result<(), Rc<dyn std::any::Any>> {
        Err(guard)
    }

    /// Takes the inner message out of `self`.
    fn take(self: Box<Self>) -> ::capnp::message::Builder<::capnp::message::HeapAllocator>;

    /// Gets the total size of the message, for flow control purposes. Although the caller
    /// could also call get_body().target_size(), doing that would walk the message tree,
    /// whereas typical implementations can compute the size more cheaply by summing
    /// segment sizes.
    fn size_in_words(&self) -> usize;
}

/// Whether the standard dispatcher releases this message before its next read.
/// Calls and Returns may retain parameter/result storage, including capabilities.
/// Unknown discriminants are short-lived and follow Unimplemented handling.
#[cfg(feature = "std")]
pub fn is_short_lived_rpc_message(body: capnp::any_pointer::Reader<'_>) -> capnp::Result<bool> {
    use capnp::traits::IntoInternalStructReader;
    // rpc.capnp Message's union tag occupies the first u16. Its wire tags are
    // Call=2 and Return=3. Classification does not need to follow the union's
    // pointer: dispatch validates that payload using its own traversal budget.
    let root = body.get_as::<crate::rpc_capnp::message::Reader>()?;
    Ok(!matches!(
        root.into_internal_struct_reader().get_data_field::<u16>(0),
        2 | 3
    ))
}

/// A message received from a [`VatNetwork`].
#[cfg(feature = "std")]
pub trait IncomingMessage {
    /// Remember the already decoded parameters of this Call. Cached views must
    /// belong to this immutable message; capability hooks are imbued by RPC.
    fn cache_params(&self, _payload: crate::rpc_capnp::payload::Reader<'_>) -> capnp::Result<()> {
        Ok(())
    }

    /// Read Call content, optionally reusing the dispatcher's checked view.
    fn get_params(&self) -> capnp::Result<capnp::any_pointer::Reader<'_>> {
        let root = self
            .get_body()?
            .get_as::<crate::rpc_capnp::message::Reader>()?;
        let crate::rpc_capnp::message::Call(call) = root.which()? else {
            return Err(Error::failed("request does not contain Call".into()));
        };
        Ok(call?.get_params()?.get_content())
    }

    /// Remember a validated Return payload from this message. The default
    /// retains compatibility with transports that decode it again on access.
    fn cache_results(&self, _payload: crate::rpc_capnp::payload::Reader<'_>) -> capnp::Result<()> {
        Ok(())
    }

    /// Read the content of a successful Return, optionally using the cached
    /// payload. Capability hooks are imbued by RPC after this call.
    fn get_results(&self) -> capnp::Result<capnp::any_pointer::Reader<'_>> {
        let root = self
            .get_body()?
            .get_as::<crate::rpc_capnp::message::Reader>()?;
        let crate::rpc_capnp::message::Return(ret) = root.which()? else {
            return Err(Error::failed("response does not contain Return".into()));
        };
        let crate::rpc_capnp::return_::Results(payload) = ret?.which()? else {
            return Err(Error::failed("response does not contain Results".into()));
        };
        Ok(payload?.get_content())
    }

    /// The original complete stream frame, when stored contiguously. These
    /// bytes must describe the same message as `get_segments`. The portable
    /// adapter validates their framing and limits before protocol admission.
    fn framed_bytes(&self) -> Option<&[u8]> {
        None
    }

    /// Borrow all original serialized segments without traversing the root.
    /// Required by the portable hosted driver adapter. Legacy custom transports
    /// may leave this unavailable while continuing to use the existing driver.
    fn get_segments(&self) -> Option<&dyn capnp::message::ReaderSegments> {
        None
    }
    /// Transfer ownership of received descriptors to RPC. Transports must bound
    /// ancillary-data reception and close truncated descriptors. RPC additionally
    /// accepts at most 253 entries and consumes each entry at most once.
    #[cfg(unix)]
    fn take_fds(&mut self) -> Vec<std::os::fd::OwnedFd> {
        Vec::new()
    }

    /// Total words in the serialized segments, including roots and unreachable data.
    /// Sum the segment lengths without traversing pointers. Incoming call flow
    /// control charges this size until the call sends its Return.
    fn size_in_words(&self) -> usize;

    /// Gets the message body, to be interpreted by the caller.
    ///
    /// The standard RPC implementation interprets it as a Message as defined
    /// in `schema/rpc.capnp`.
    fn get_body(&self) -> ::capnp::Result<::capnp::any_pointer::Reader<'_>>;
}

/// Exclusive, caller-polled reception for a message transport.
#[cfg(feature = "std")]
pub trait IncomingMessageStream {
    /// Poll one message; Pending retains partial framing progress. The stream
    /// owns input exclusively. The connection's factory specifies whether
    /// dropping the stream ends reception or returns input to the connection.
    fn poll_receive(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<capnp::Result<Option<Box<dyn IncomingMessage>>>>;
}

/// Exclusive reception of standard frames and their ancillary resources.
/// A successful frame remains immutable and available until `finish_frame`;
/// polling again before finishing returns that same frame, without reading IO.
/// Dropping the stream ends reception. Partial progress survives Pending.
#[cfg(feature = "std")]
pub trait IncomingFrameStream {
    /// A complete next frame is available without IO or transferring ownership.
    /// This hint permits bounded reply batching; false always remains valid.
    fn has_buffered_next_frame(&self) -> bool {
        false
    }
    fn poll_frame(&mut self, cx: &mut Context<'_>) -> Poll<capnp::Result<Option<&[u8]>>>;
    fn frame(&self) -> Option<&[u8]>;
    /// Retain the current frame's immutable byte allocation and exact range.
    /// The allocation may contain other bytes; consumers must charge its full
    /// capacity. No input is consumed. After finish_frame the stream must keep
    /// this owner immutable and detach before any conflicting read or edit.
    fn share_frame(&mut self) -> Option<(std::sync::Arc<Vec<u8>>, core::ops::Range<usize>)> {
        None
    }
    /// Prepare a complete frame or a validated streaming length. The default
    /// retains complete-frame behavior. Streaming implementations expose no
    /// frame slice until complete and implement `poll_read_frame` below.
    fn poll_frame_size(&mut self, cx: &mut Context<'_>) -> Poll<capnp::Result<Option<usize>>> {
        self.poll_frame(cx).map_ok(|frame| frame.map(<[u8]>::len))
    }
    /// Read an incomplete prepared frame into caller-owned storage, without
    /// crossing its boundary. Pending must retain progress and register a wake.
    fn poll_read_frame(&mut self, _: &mut Context<'_>, _: &mut [u8]) -> Poll<capnp::Result<usize>> {
        Poll::Ready(Err(Error::unimplemented(
            "stream does not support partial frames".into(),
        )))
    }
    /// Release the current frame. An adapter abandoning incomplete input must
    /// stop polling this stream and retain it until output drain has finished.
    fn finish_frame(&mut self);
    /// Transfer descriptors belonging to the complete current frame, once.
    /// No descriptor from a prefetched successor may be returned. Untaken
    /// descriptors are released by finish_frame. Partial reads retain them.
    #[cfg(unix)]
    fn take_fds(&mut self) -> Vec<std::os::fd::OwnedFd> {
        Vec::new()
    }
}

#[cfg(feature = "std")]
/// Reusable frame publication into a connection's existing ordered output.
/// Acceptance is synchronous and may defer IO until the caller ends its bounded
/// publication turn with poll_flush. That method drives or schedules output and
/// observes physical completion of all
/// previously accepted frames. Implementations retain frame resources through
/// that completion and never cancel accepted writes when this handle is dropped.
pub trait OutgoingMessageStream {
    /// Stop accepting frames and drain/close the physical write half. Enqueue
    /// shutdown synchronously; dropping its observer must not cancel it.
    fn shutdown(&mut self, result: capnp::Result<()>) -> Promise<(), Error>;
    fn send(&mut self, frame: OutgoingFrame) -> Result<(), Error>;
    fn poll_flush(
        &mut self,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Result<(), Error>>;
}

/// Exclusive hosted frame transport. outgoing.poll_flush must arrange output
/// progress and retain accepted resources until local physical completion.
#[cfg(feature = "std")]
pub struct FrameIo {
    pub incoming: Box<dyn IncomingFrameStream>,
    pub outgoing: Box<dyn OutgoingMessageStream>,
    /// Independent physical-writer failure, when the transport has such a task.
    pub write_finished: Option<Promise<(), Error>>,
}

/// Immutable control-plane capability selected when a connection is admitted.
/// TwoParty never enables introductions, answer handoffs or multiparty Join.
/// Networks whose features may activate later must select Network from the start.
#[cfg(feature = "std")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlMode {
    TwoParty,
    Network,
}

/// A two-way RPC connection, created by [`VatNetwork::connect()`].
#[cfg(feature = "std")]
pub trait Connection<VatId> {
    /// This value is fixed for the lifetime of this transport connection.
    fn control_mode(&self) -> ControlMode;

    /// Atomically transfer the portable connection's exclusive frame owners.
    /// A second acquisition fails without changing existing owners. Portable
    /// hosting does not negotiate per-message fallback interfaces.
    fn take_frame_io(&mut self) -> capnp::Result<FrameIo>;

    /// Optionally lend exclusive input to a reusable poller with the same
    /// message lifetime rules as `receive_incoming_message`: Calls and Returns
    /// remain valid across subsequent reads. Dropping the poller must return
    /// input to the connection, preserving partial progress and keeping the
    /// transport alive through output drain. Custom transports may return None
    /// to retain the per-message future interface.
    fn take_owned_incoming_stream(&mut self) -> Option<Box<dyn IncomingMessageStream>> {
        None
    }

    /// Returns the peer identity established by the network. When used for
    /// bootstrap authorization, the network must authenticate this identity
    /// before delivering the connection to the RPC system.
    fn get_peer_vat_id(&self) -> VatId;

    /// Stable identity of this live connection within its VatNetwork. Handles
    /// for the same connection must return the same ID; distinct connections
    /// must return distinct IDs, even when they authenticate the same vat.
    fn connection_id(&self) -> usize;

    /// Allocates a new message to be sent on this connection.
    ///
    /// If `first_segment_word_size` is non-zero, it should be treated as a
    /// hint suggesting how large to make the first segment.  This is entirely
    /// a hint and the connection may adjust it up or down.  If it is zero,
    /// the connection should choose the size itself.
    fn new_outgoing_message(&mut self, first_segment_word_size: u32) -> Box<dyn OutgoingMessage>;

    /// Allocate an initialized Call envelope. Implementations may copy a
    /// validated template; each message must own independent mutable storage.
    fn new_outgoing_call(
        &mut self,
        first_segment_word_size: u32,
    ) -> Result<Box<dyn OutgoingMessage>, Error> {
        let mut message = self.new_outgoing_message(first_segment_word_size);
        message.init_call()?;
        Ok(message)
    }

    /// Allocate an initialized Return envelope with independent mutable storage.
    fn new_outgoing_return(
        &mut self,
        first_segment_word_size: u32,
    ) -> Result<Box<dyn OutgoingMessage>, Error> {
        let mut message = self.new_outgoing_message(first_segment_word_size);
        message
            .get_body()?
            .init_as::<crate::rpc_capnp::message::Builder>()
            .init_return();
        Ok(message)
    }

    /// Build a standard Finish. Its identity and release flag are supplied for
    /// every message; transports may reuse an immutable envelope template.
    fn new_outgoing_finish(
        &mut self,
        question_id: u32,
        release_result_caps: bool,
    ) -> Result<Box<dyn OutgoingMessage>, Error> {
        let mut message = self.new_outgoing_message(5);
        let mut finish = message
            .get_body()?
            .init_as::<crate::rpc_capnp::message::Builder>()
            .init_finish();
        finish.set_question_id(question_id);
        finish.set_release_result_caps(release_result_caps);
        finish.set_require_early_cancellation_workaround(false);
        Ok(message)
    }

    /// Enqueue an immutable wire frame in the same FIFO as ordinary messages.
    /// This explicit submission preserves capability indices without decoding
    /// into a hook-imbued builder. Portable hosting uses FrameIo's reusable stream.
    /// Retain bytes and resources until the
    /// write completes, even if the returned completion promise is dropped.
    /// Completion means local write completion, never peer receipt.
    fn send_frame(&mut self, _frame: OutgoingFrame) -> Promise<(), Error> {
        Promise::err(Error::unimplemented(
            "transport does not support portable RPC frames".into(),
        ))
    }

    /// Waits for a message to be received and returns it.  If the read stream cleanly terminates,
    /// returns None. If any other problem occurs, returns an Error.
    fn receive_incoming_message(&mut self) -> Promise<Option<Box<dyn IncomingMessage>>, Error>;

    /// Receive a message which the caller will drop before starting its next
    /// receive. Buffered transports may lend immutable receive storage even for
    /// Calls and Returns. Keeping that storage alive must remain memory-safe;
    /// a transport may reject the next receive until the message is dropped.
    /// This message-oriented API is independent of the exclusive FrameIo input.
    /// The default receives an owned message.
    fn receive_incoming_message_short_lived(
        &mut self,
    ) -> Promise<Option<Box<dyn IncomingMessage>>, Error> {
        self.receive_incoming_message()
    }

    /// Optional cooperative polling of the existing output driver. This is a
    /// second observer of the same FIFO, never a separate writer. It must not
    /// keep the network alive or require a Connection borrow while polled.
    /// The ordinary network driver remains responsible for progress and close.
    /// FrameIo's output stream owns cooperative flushing for portable hosting.
    fn output_driver(&self) -> Option<Promise<(), Error>> {
        None
    }

    /// Observe termination of the outgoing write queue, independently of input
    /// EOF or closing the transport. Errors disconnect pending RPC calls even
    /// when a failed transport's close operation blocks. Success means queued
    /// writes finished, not that the output is closed or the peer received them.
    /// The observer must not retain the connection. Networks that report write
    /// errors through another driver may leave this unsupported.
    fn when_write_finished(&self) -> Option<Promise<(), Error>> {
        None
    }

    /// Notifies the network when RPC protocol tables become empty or active.
    /// Initially called with false. While idle, no RPC messages will be sent
    /// until incoming traffic or a network connect/accept result reactivates the
    /// connection. The network may use this to close unused connections after
    /// flushing queued output. Notifications are deduplicated; the default is
    /// a no-op. In Rust, idle checks run after the current RPC operation unwinds.
    fn set_idle(&mut self, _idle: bool) {}

    /// Whether this network implements authenticated third-party rendezvous.
    fn supports_third_party(&self) -> bool {
        false
    }

    /// This connection uses the network's authenticated independent-share Join profile.
    fn supports_multiparty_join(&self) -> bool {
        false
    }

    /// This connection is a bilateral network boundary and uses the JoinKeyPart
    /// and JoinResult encoding from rpc-twoparty.capnp. Do not enable this for
    /// a general multiparty connection: all join parts must cross the boundary.
    fn supports_two_party_join(&self) -> bool {
        false
    }

    /// Whether the peer can consume `sendResultsTo.yourself` followed by
    /// `takeFromOtherQuestion`. Standard two-party streams conservatively use
    /// ordinary forwarding: capnp-rpc 0.27.0 can stall on this optimization.
    /// Custom networks retain standard tail transfer unless they override this.
    /// This is local knowledge of the peer, not a wire-level negotiation.
    fn supports_tail_call_transfer(&self) -> bool {
        true
    }

    /// Supports authenticated, single-use ThirdPartyAnswer rendezvous through
    /// the third-party exchange hooks. Independent from capability introductions.
    fn supports_third_party_answers(&self) -> bool {
        false
    }
    /// Opt in to migrating already-used caller pipeline references through an
    /// authenticated multiparty Join of the old and adopted paths. Join shares
    /// must follow earlier calls through transparent proxies; opaque boundaries
    /// retain their equality policy. Requires a bounded setup deadline.
    fn supports_pipeline_join_fence(&self) -> bool {
        false
    }

    /// Deadline for one third-party answer rendezvous, starting at this call.
    /// Resolving successfully expires setup; returning an error fails setup as
    /// a disconnection with that diagnostic. The runtime drops the deadline once authentication matches,
    /// so it never limits application method execution. It also drops it on
    /// cancellation. Implementations must not retain the connection in the timer.
    /// The default leaves timing policy to the application.
    fn third_party_answer_timeout(&self) -> Promise<(), Error> {
        Promise::from_future(futures::future::pending())
    }

    /// Creates network-specific recipient and contact tokens for an introduction
    /// from this connection's peer to `recipient`. Return false to use a proxy.
    fn introduce_to(
        &mut self,
        _recipient: VatId,
        _contact: capnp::any_pointer::Builder<'_>,
        _await_token: capnp::any_pointer::Builder<'_>,
    ) -> capnp::Result<bool> {
        Ok(false)
    }

    /// Delegates a received contact to another authenticated vat without accepting
    /// it locally. Return false when the network requires a proxy instead. The
    /// result must authorize only `recipient` and refer to the same provision.
    fn forward_third_party_to_contact(
        &mut self,
        _contact: capnp::any_pointer::Reader<'_>,
        _recipient: VatId,
        _result: capnp::any_pointer::Builder<'_>,
    ) -> capnp::Result<bool> {
        Ok(false)
    }

    /// Allocates a nonempty embargo ID unique across all recipients accepting a
    /// provision, including forwarded contacts. A connection-local counter is
    /// insufficient. Networks implementing native introductions must override
    /// this method (for example with authenticated vat identity plus a counter).
    fn generate_embargo_id(&mut self) -> capnp::Result<Vec<u8>> {
        Err(Error::unimplemented(
            "network embargo IDs unavailable".into(),
        ))
    }

    /// Authenticates an introduction received on this connection and creates or
    /// reuses a connection to its host. Writes the token for Accept.provision.
    /// Returning None means the contact refers to this vat itself; the runtime
    /// will call `complete_third_party_local()` with the completion token.
    fn connect_to_introduced(
        &mut self,
        _contact: capnp::any_pointer::Reader<'_>,
        _completion: capnp::any_pointer::Builder<'_>,
    ) -> capnp::Result<Option<Box<dyn Connection<VatId>>>> {
        Err(Error::unimplemented(
            "third-party connections unavailable".into(),
        ))
    }

    /// Completes a self-introduction. Authenticate the token for the local vat,
    /// rather than for this connection's peer. Like remote completion, this
    /// must support completion before Provide and cancellation of the waiter.
    fn complete_third_party_local(
        &mut self,
        _provision: capnp::any_pointer::Reader<'_>,
    ) -> Promise<Rc<third_party::ThirdPartyExchange>, Error> {
        Promise::err(Error::unimplemented(
            "local third-party rendezvous unavailable".into(),
        ))
    }

    /// Registers a provision until the returned guard is dropped. The network
    /// must bind `recipient` to the authenticated recipient, not merely a name
    /// supplied by an untrusted peer. May be called before or after completion.
    fn await_third_party(
        &mut self,
        _recipient: capnp::any_pointer::Reader<'_>,
        _value: Rc<third_party::ThirdPartyExchange>,
    ) -> capnp::Result<Box<dyn std::any::Any>> {
        Err(Error::unimplemented(
            "third-party rendezvous unavailable".into(),
        ))
    }

    /// Retrieves a matching provision for this authenticated connection.
    /// Implementations must handle Accept arriving before Provide. Dropping the
    /// returned promise must cancel its waiter.
    fn complete_third_party(
        &mut self,
        _provision: capnp::any_pointer::Reader<'_>,
    ) -> Promise<Rc<third_party::ThirdPartyExchange>, Error> {
        Promise::err(Error::unimplemented(
            "third-party rendezvous unavailable".into(),
        ))
    }

    /// Constructs a flow controller for a new stream on this connection.
    ///
    /// Returns (fc, p), where fc is the new flow controller and p is a promise
    /// that must be polled in order to drive the flow controller.
    fn new_stream(&mut self) -> (Box<dyn FlowController>, Promise<(), Error>) {
        new_fixed_window_flow_controller(crate::flow_control::DEFAULT_WINDOW_SIZE)
    }

    /// Waits until all outgoing messages have been sent, then shuts down the outgoing stream. The
    /// returned promise resolves after shutdown is complete.
    fn shutdown(&mut self, result: ::capnp::Result<()>) -> Promise<(), Error>;
}

/// Creates a byte-window stream controller and its acknowledgement driver.
/// Poll the driver until it completes. A successful send grants flow credit;
/// it does not acknowledge delivery. Dropping the controller releases blocked
/// senders; the driver retains outstanding acknowledgements until they finish.
#[cfg(feature = "std")]
pub fn new_fixed_window_flow_controller(
    window_size: usize,
) -> (Box<dyn FlowController>, Promise<(), Error>) {
    let (controller, driver) =
        crate::flow_control::WindowFlowController::new(window_size, None, None);
    (Box::new(controller), driver)
}

/// Creates a controller that samples a shared byte-window estimate on sends and
/// successful acknowledgements. The getter is owned by the controller/driver.
/// Queries are skipped after failure and while in-flight bytes fit within the
/// largest message, matching C++'s readiness short-circuit.
/// Changing the estimate alone does not wake blocked sends; the next successful
/// acknowledgement rechecks them. At zero, credit remains available while
/// in-flight bytes do not exceed the largest message observed.
#[cfg(feature = "std")]
pub fn new_variable_window_flow_controller(
    get_window: impl Fn() -> usize + 'static,
) -> (Box<dyn FlowController>, Promise<(), Error>) {
    let (controller, driver) =
        crate::flow_control::WindowFlowController::new(0, Some(Rc::new(get_window)), None);
    (Box::new(controller), driver)
}

/// Creates the C++ adaptive RPC streaming controller. It estimates bandwidth and
/// minimum RTT from application acknowledgements, probes with 2x startup growth,
/// then uses 5/4 growth and 7/8 decay collars. Estimated windows are bounded to
/// 64 KiB..=1 GiB; the supplied initial window is used until an estimate exists.
/// 256 KiB is a useful initial value. Poll the returned acknowledgement driver.
#[cfg(feature = "std")]
pub fn new_adaptive_flow_controller(
    initial_window: usize,
) -> (Box<dyn FlowController>, Promise<(), Error>) {
    let origin = std::time::Instant::now();
    new_adaptive_flow_controller_with_clock(initial_window, move || origin.elapsed())
}

/// Adaptive streaming with an application-supplied monotonic clock, expressed
/// as nondecreasing durations from any fixed origin. Useful with virtual clocks
/// or executors that supply their own time. Estimates have microsecond precision;
/// zero-length intervals do not update the window. Clock sampling requires no
/// timer task and does not depend on a particular async runtime.
#[cfg(feature = "std")]
pub fn new_adaptive_flow_controller_with_clock(
    initial_window: usize,
    clock: impl Fn() -> std::time::Duration + 'static,
) -> (Box<dyn FlowController>, Promise<(), Error>) {
    let (controller, driver) =
        crate::flow_control::WindowFlowController::new(initial_window, None, Some(Rc::new(clock)));
    (Box::new(controller), driver)
}

/// Tracks a particular RPC stream in order to implement a flow control algorithm.
#[cfg(feature = "std")]
pub trait FlowController {
    fn send(
        &mut self,
        message: Box<dyn OutgoingMessage>,
        ack: Promise<(), Error>,
    ) -> Promise<(), Error>;
    fn wait_all_acked(&mut self) -> Promise<(), Error>;
}

/// Network facility between vats, it determines how to form connections between
/// vats.
///
/// ## Vat
///
/// Cap'n Proto RPC operates between vats, where a "vat" is some sort of host of
/// objects.  Typically one Cap'n Proto process (in the Unix sense) is one vat.
#[cfg(feature = "std")]
pub trait VatNetwork<VatId> {
    /// Transfer physical output to the portable runtime's final phase. After
    /// extraction, connection publication only queues owned frames; lifecycle
    /// progress must observe completion without polling this output task.
    /// Return None only when the network has no physical output driver to move
    /// (for example, an in-memory transport or an externally driven network).
    /// Implementations must declare this ownership explicitly.
    fn take_output_driver(&mut self) -> Option<Promise<(), Error>>;

    /// Schedule a deferred network turn. The callback must arrange a later
    /// wake of this context without polling the RPC system reentrantly. Hosts
    /// may use their executor's yield queue so application tasks and IO can run
    /// before output is batched. The default uses an ordinary wake.
    fn task_yield(&self) -> fn(&mut Context<'_>) {
        |cx| cx.waker().wake_by_ref()
    }

    fn join_network(&self) -> Option<Rc<dyn multiparty::JoinNetwork<VatId>>> {
        None
    }

    /// Connects to `host_id`.
    ///
    /// Returns None if `host_id` refers to the local vat.
    fn connect(&mut self, host_id: VatId) -> Option<Box<dyn Connection<VatId>>>;

    /// Waits for the next incoming connection and return it.
    fn accept(&mut self) -> Promise<Box<dyn Connection<VatId>>, ::capnp::Error>;

    /// A promise that cannot be resolved until the shutdown.
    fn drive_until_shutdown(&mut self) -> Promise<(), Error>;
}

/// Constructs the capability exposed by each Bootstrap request.
///
/// The peer identity comes from the authenticated [`Connection`], never from
/// Bootstrap message content. The factory runs once per request, so it may
/// apply current application policy. Previously issued capabilities retain
/// their authority unless the application separately revokes them.
#[cfg(feature = "std")]
pub trait BootstrapFactory<VatId> {
    fn create_for(&self, client_id: &VatId) -> capnp::Result<capnp::capability::Client>;
}

#[cfg(feature = "std")]
pub(crate) enum Bootstrap<VatId> {
    Static(Box<dyn ClientHook>),
    Factory {
        factory: Rc<dyn BootstrapFactory<VatId>>,
        local_id: Rc<VatId>,
    },
}
#[cfg(feature = "std")]
impl<VatId> Clone for Bootstrap<VatId> {
    fn clone(&self) -> Self {
        match self {
            Self::Static(cap) => Self::Static(cap.clone()),
            Self::Factory { factory, local_id } => Self::Factory {
                factory: factory.clone(),
                local_id: local_id.clone(),
            },
        }
    }
}
#[cfg(feature = "std")]
impl<VatId> Bootstrap<VatId> {
    fn for_peer(&self, peer: &VatId) -> capnp::Result<Box<dyn ClientHook>> {
        match self {
            Self::Static(cap) => Ok(cap.clone()),
            Self::Factory { factory, .. } => factory.create_for(peer).map(|cap| cap.hook),
        }
    }
    fn local(&self) -> capnp::Result<Box<dyn ClientHook>> {
        match self {
            Self::Static(cap) => Ok(cap.clone()),
            Self::Factory { local_id, .. } => self.for_peer(local_id),
        }
    }
}

/// A portal to objects available on the network.
///
/// The RPC implementation sits on top of an implementation of [`VatNetwork`], which
/// determines how to form connections between vats. The RPC implementation determines
/// how to use such connections to manage object references and make method calls.
///
/// Multiple connections and native third-party handoff are supported when the
/// network implements the authenticated rendezvous and introduction hooks.
/// [`twoparty::VatNetwork`] remains available for bilateral connections.
///
/// An `RpcSystem` is a non-`Send`able `Future` and needs to be driven by a task
/// executor. A common way accomplish that is to pass the `RpcSystem` to
/// `tokio::task::spawn_local()`.
#[must_use = "futures do nothing unless polled"]
#[cfg(feature = "std")]
pub struct RpcSystem<VatId>
where
    VatId: 'static,
{
    network: Rc<RefCell<Box<dyn crate::VatNetwork<VatId>>>>,

    bootstrap: Bootstrap<VatId>,

    connections: Rc<RefCell<crate::system::ConnectionRegistry<VatId>>>,
    join_context: Rc<rpc::JoinContext<VatId>>,

    tasks: TaskSet<Error>,
    handle: crate::task_set::TaskSetHandle<Error>,
}

#[cfg(feature = "std")]
impl<VatId> RpcSystem<VatId> {
    /// Constructs a new `RpcSystem` with the given network and bootstrap capability.
    pub fn new(
        network: Box<dyn crate::VatNetwork<VatId>>,
        bootstrap: Option<::capnp::capability::Client>,
    ) -> Self {
        let bootstrap_cap = match bootstrap {
            Some(cap) => cap.hook,
            None => broken::new_cap(Error::failed("no bootstrap capability".to_string())),
        };
        Self::with_bootstrap(network, Bootstrap::Static(bootstrap_cap))
    }

    /// Constructs a system with a bootstrap factory selected by authenticated peer.
    /// `local_vat_id` must identify this network's local vat; it is used when
    /// `bootstrap()` connects to self. No `Clone` bound is imposed on VatId.
    /// Factory errors become Bootstrap exceptions (or a broken local client),
    /// leaving the connection available for later requests.
    pub fn new_with_bootstrap_factory(
        network: Box<dyn crate::VatNetwork<VatId>>,
        local_vat_id: VatId,
        factory: Rc<dyn BootstrapFactory<VatId>>,
    ) -> Self {
        Self::with_bootstrap(
            network,
            Bootstrap::Factory {
                factory,
                local_id: Rc::new(local_vat_id),
            },
        )
    }

    fn with_bootstrap(
        mut network: Box<dyn crate::VatNetwork<VatId>>,
        bootstrap: Bootstrap<VatId>,
    ) -> Self {
        let (mut handle, mut tasks) = TaskSet::new(Box::new(SystemTaskReaper));
        tasks.set_defer(network.task_yield());

        system::drive_network(&mut *network, &mut handle);

        let join_network = network.join_network();
        let join_context = Rc::new(rpc::JoinContext {
            bootstrap: bootstrap.clone(),
            tasks: handle.clone(),
        });
        let mut result = Self {
            join_context,
            network: Rc::new(RefCell::new(network)),
            bootstrap,
            connections: Rc::new(RefCell::new(crate::system::ConnectionRegistry::new())),

            tasks,
            handle: handle.clone(),
        };

        result.connections.borrow_mut().join_network = join_network;
        result.connections.borrow_mut().join_context = Rc::downgrade(&result.join_context);
        let accept_loop = result.accept_loop();
        handle.add(accept_loop);
        result
    }

    /// Connects to the given vat and returns its bootstrap interface, returns
    /// a client that can be used to invoke the bootstrap interface.
    pub fn bootstrap<T>(&mut self, vat_id: VatId) -> T
    where
        T: ::capnp::capability::FromClientHook,
    {
        self.get_bootstrapper().bootstrap(vat_id)
    }

    /// Obtain typed bootstraps while the system is running on another local
    /// task. The handle does not keep the system, transport or bootstrap alive.
    /// After shutdown, requests return broken clients with Disconnected errors.
    pub fn get_bootstrapper(&self) -> Bootstrapper<VatId> {
        Bootstrapper {
            network: Rc::downgrade(&self.network),
            connections: Rc::downgrade(&self.connections),
            context: Rc::downgrade(&self.join_context),
        }
    }

    /// A handle for capability equality through local resolution, bilateral
    /// boundaries or authenticated independently routed Join shares. Drive this RPC system while awaiting remote joins.
    pub fn get_joiner(&self) -> Joiner<VatId> {
        Joiner::new(Rc::downgrade(&self.connections))
    }

    fn accept_loop(&mut self) -> Promise<(), Error> {
        let connections = self.connections.clone();
        let bootstrap = self.bootstrap.clone();
        let handle = self.handle.clone();
        let network = self.network.clone();
        Promise::from_future(async move {
            loop {
                // Do not hold a RefCell borrow while waiting on the network.
                let accepting = network.borrow_mut().accept();
                let connection = accepting.await?;
                if connections.borrow().is_closing() {
                    return Ok(());
                }
                Self::get_connection_state(
                    &connections,
                    bootstrap.clone(),
                    connection,
                    handle.clone(),
                )?;
            }
        })
    }

    pub(crate) fn get_connection_state(
        connections: &Rc<RefCell<crate::system::ConnectionRegistry<VatId>>>,
        bootstrap: Bootstrap<VatId>,
        connection: Box<dyn crate::Connection<VatId>>,
        handle: crate::task_set::TaskSetHandle<Error>,
    ) -> capnp::Result<Rc<rpc::ConnectionState<VatId>>> {
        system::connect(connections, bootstrap, connection, handle)
    }

    /// Encodes optional exception trace text sent to peers. Disabled by default.
    /// The callback applies at serialization time to existing and future
    /// connections, including introduced connections. Rust errors do not
    /// capture a stack automatically; the application supplies the diagnostic.
    pub fn set_trace_encoder(&mut self, encoder: impl Fn(&Error) -> String + 'static) {
        self.connections.borrow_mut().trace_encoder = Some(Rc::new(encoder));
    }

    /// Stop including diagnostic traces in subsequently serialized exceptions.
    pub fn clear_trace_encoder(&mut self) {
        self.connections.borrow_mut().trace_encoder = None;
    }

    /// Sets the per-connection incoming call limit, in words, for existing and
    /// future connections. The default is `usize::MAX` (unlimited).
    ///
    /// Like C++, the reader pauses above the limit and resumes strictly below
    /// it. All incoming messages pause, including Return and Finish, so calls
    /// waiting on the peer can deadlock. A zero limit needs a later increase
    /// to resume after a call exceeds it. This is backpressure, not a hard
    /// memory bound: the call that crosses the threshold is still dispatched.
    pub fn set_flow_limit(&mut self, words: usize) {
        let states = {
            let mut registry = self.connections.borrow_mut();
            registry.flow_limit = words;
            registry.states()
        };
        for state in states {
            state.set_flow_limit(words);
        }
    }

    /// Limit outgoing Calls per connection, for existing and future connections.
    /// Default: unlimited. Zero rejects new calls. Reducing the limit does not
    /// cancel accepted work. Excess requests fail locally with Overloaded before
    /// a Call is sent; there is no hidden admission wait queue.
    ///
    /// Credit is reserved when a wire request is created and retained through
    /// its question lifetime and local write completion. Holding a response or
    /// pipeline can therefore hold credit. Streaming and tail calls share this
    /// limit. Bootstrap and protocol progress messages remain unrestricted.
    ///
    /// This is a count limit, not a memory bound: unsent payload allocation,
    /// incoming work and purely local unresolved capabilities need separate
    /// budgets. Apply a limit to every RPC system used by the application.
    pub fn set_outgoing_call_limit(&mut self, calls: usize) {
        let states = {
            let mut registry = self.connections.borrow_mut();
            registry.outgoing_call_limit = calls;
            registry.states()
        };
        for state in states {
            state.set_outgoing_call_limit(calls);
        }
    }

    pub fn diagnostics(&self) -> RpcDiagnostics {
        let registry = Rc::downgrade(&self.connections);
        RpcDiagnostics(Rc::new(move || {
            let Some(registry) = registry.upgrade() else {
                return Vec::new();
            };
            let states = registry.borrow().states();
            let mut snapshots = states
                .into_iter()
                .map(|state| state.snapshot())
                .collect::<Vec<_>>();
            snapshots.sort_by_key(|snapshot| snapshot.connection_id);
            snapshots
        }))
    }

    /// Returns a `Disconnector` future that can be run to cleanly close the connection to this `RpcSystem`'s network.
    /// You should get the `Disconnector` before you spawn the `RpcSystem`.
    pub fn get_disconnector(&self) -> Disconnector<VatId> {
        Disconnector::new(self.connections.clone())
    }
}

#[cfg(feature = "std")]
impl<VatId: 'static> Drop for RpcSystem<VatId> {
    fn drop(&mut self) {
        let states = system::ConnectionRegistry::begin_close(&self.connections);
        for state in states {
            state.disconnect(Error::disconnected("RPC system was dropped".into()));
        }
    }
}

#[cfg(feature = "std")]
impl<VatId> Future for RpcSystem<VatId>
where
    VatId: 'static,
{
    type Output = Result<(), Error>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        Pin::new(&mut self.tasks).poll(cx)
    }
}

/// Portable owner for non-cancellable local methods. Poll the returned driver on
/// your local executor. Dropping it releases the executor's task ownership;
/// calls with no remaining caller are then canceled.
/// Incoming RPC calls use their RpcSystem's task owner automatically.
#[cfg(feature = "std")]
pub fn new_call_executor() -> (Rc<dyn capnp::capability::CallExecutor>, Promise<(), Error>) {
    struct Reaper;
    impl task_set::TaskReaper<Error> for Reaper {
        fn task_failed(&mut self, _error: Error) {}
    }
    let (handle, driver) = task_set::TaskSet::new(Box::new(Reaper));
    (
        Rc::new(CallTaskExecutor(handle)),
        Promise::from_future(driver),
    )
}

#[cfg(feature = "std")]
pub(crate) struct CallTaskExecutor(task_set::TaskSetHandle<Error>);
#[cfg(feature = "std")]
impl capnp::capability::CallExecutor for CallTaskExecutor {
    fn spawn(&self, task: Promise<(), Error>) -> capnp::Result<()> {
        self.0
            .try_add(async move {
                let _ = task.await;
                Ok(())
            })
            .map_err(|_| Error::disconnected("call executor has shut down".into()))
    }
}

/// Construct a local server whose non-cancellable calls can outlive their local
/// callers. Keep the executor driver running until these calls have completed.
#[cfg(feature = "std")]
pub fn new_client_with_executor<C, S>(
    server: S,
    executor: Rc<dyn capnp::capability::CallExecutor>,
) -> C
where
    C: capnp::capability::FromServer<S>,
{
    new_client_from_rc_with_executor(Rc::new(server), executor)
}

/// Variant of [`new_client_with_executor`] for a shared server.
#[cfg(feature = "std")]
pub fn new_client_from_rc_with_executor<C, S>(
    server: Rc<S>,
    executor: Rc<dyn capnp::capability::CallExecutor>,
) -> C
where
    C: capnp::capability::FromServer<S>,
{
    let client = local::Client::new_with_executor(C::from_server(server), executor);
    capnp::capability::FromClientHook::new(client.into_hook())
}

/// Creates a new local RPC client of type `C` out of an object that implements a server trait `S`.
///
/// Generated methods without `allowCancellation` need an independent task owner.
/// Incoming RPC calls obtain one from their `RpcSystem`. For calls made directly
/// to this local client, use [`new_client_with_executor`] instead. Without an
/// owner, protected calls fail before their application future is polled.
#[cfg(feature = "std")]
pub fn new_client<C, S>(s: S) -> C
where
    C: capnp::capability::FromServer<S>,
{
    new_client_from_rc(Rc::new(s))
}

/// Construct a server from an owned runtime schema. Protected direct calls
/// require new_loaded_client_from_rc() with an executor.
#[cfg(feature = "std")]
pub fn new_loaded_client<S: capnp::schema_loader::dynamic::Server>(
    server: S,
    schema: capnp::schema_loader::dynamic::ServiceSchema,
) -> capnp::capability::Client {
    new_loaded_client_from_rc(Rc::new(server), schema, None)
}

#[cfg(feature = "std")]
pub fn new_loaded_client_from_rc<S: capnp::schema_loader::dynamic::Server>(
    server: Rc<S>,
    schema: capnp::schema_loader::dynamic::ServiceSchema,
    executor: Option<Rc<dyn capnp::capability::CallExecutor>>,
) -> capnp::capability::Client {
    let client =
        local::Client::new(capnp::schema_loader::dynamic::ServerDispatch { server, schema });
    let client = match executor {
        Some(executor) => client.with_executor(executor),
        None => client,
    };
    capnp::capability::Client::new(client.into_hook())
}

/// Construct a reflected server, preserving its compiled interface metadata.
/// Dynamic servers protect calls by default; supply an executor for protected
/// direct local calls, or override Server::allow_cancellation().
#[cfg(feature = "std")]
pub fn new_dynamic_client<S: capnp::dynamic_capability::Server>(
    server: S,
) -> capnp::dynamic_capability::Client {
    new_dynamic_client_from_rc(Rc::new(server), None)
}

/// Shared-server variant with an optional owner for protected local calls.
#[cfg(feature = "std")]
pub fn new_dynamic_client_from_rc<S: capnp::dynamic_capability::Server>(
    server: Rc<S>,
    executor: Option<Rc<dyn capnp::capability::CallExecutor>>,
) -> capnp::dynamic_capability::Client {
    let schema = server.get_schema();
    let client = local::Client::new(capnp::dynamic_capability::ServerDispatch { server, schema });
    let client = match executor {
        Some(executor) => client.with_executor(executor),
        None => client,
    };
    capnp::dynamic_capability::Client::new(
        capnp::capability::Client::new(client.into_hook()),
        schema,
    )
}

/// Export a local server with a Unix file descriptor. Calls continue to work
/// over transports which discard descriptors. The descriptor is owned by the
/// capability and retained by any outgoing messages still waiting to be written.
#[cfg(unix)]
#[cfg(feature = "std")]
pub fn new_fd_client<C, S>(server: S, fd: std::os::fd::OwnedFd) -> C
where
    C: capnp::capability::FromServer<S>,
{
    let client = local::Client::new_with_fd(
        <C as capnp::capability::FromServer<S>>::from_server(Rc::new(server)),
        Rc::new(fd),
    );
    capnp::capability::FromClientHook::new(client.into_hook())
}

/// Variant of [`new_fd_client`] that also owns protected direct local calls.
#[cfg(unix)]
#[cfg(feature = "std")]
pub fn new_fd_client_with_executor<C, S>(
    server: S,
    fd: std::os::fd::OwnedFd,
    executor: Rc<dyn capnp::capability::CallExecutor>,
) -> C
where
    C: capnp::capability::FromServer<S>,
{
    let client = local::Client::new_with_fd(C::from_server(Rc::new(server)), Rc::new(fd))
        .with_executor(executor);
    capnp::capability::FromClientHook::new(client.into_hook())
}

/// Variant of `new_client` that works on an `Rc<S>`.
/// Repeated construction for the same server and generated interface shares
/// identity, streaming state and the first client's executor/descriptor options.
/// Construction fails with a broken capability if this server already has a live
/// revocable client, including through another generated interface view.
#[cfg(feature = "std")]
pub fn new_client_from_rc<C, S>(s: Rc<S>) -> C
where
    C: capnp::capability::FromServer<S>,
{
    capnp::capability::FromClientHook::new(
        local::Client::new(<C as capnp::capability::FromServer<S>>::from_server(s)).into_hook(),
    )
}

/// Collection of unwrappable capabilities.
///
/// Allows a server to recognize its own capabilities when passed back to it, and obtain the
/// underlying Server objects associated with them. Holds only weak references to Server objects
/// allowing Server objects to be dropped when dropped by the remote client. Call the `gc` method
/// to reclaim memory used for Server objects that have been dropped.
#[cfg(feature = "std")]
pub struct CapabilityServerSet<S, C>
where
    C: capnp::capability::FromServer<S>,
{
    caps: std::collections::HashMap<usize, Weak<S>>,
    marker: std::marker::PhantomData<C>,
}

#[cfg(feature = "std")]
impl<S, C> Default for CapabilityServerSet<S, C>
where
    C: capnp::capability::FromServer<S>,
{
    fn default() -> Self {
        Self {
            caps: std::default::Default::default(),
            marker: std::marker::PhantomData,
        }
    }
}

#[cfg(feature = "std")]
impl<S, C> CapabilityServerSet<S, C>
where
    C: capnp::capability::FromServer<S>,
{
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a new capability to the set and returns a client backed by it.
    pub fn new_client(&mut self, s: S) -> C {
        self.new_client_from_rc(Rc::new(s))
    }

    /// Variant of `new_client` that works on an `Rc<S>`.
    pub fn new_client_from_rc(&mut self, rc: Rc<S>) -> C {
        let weak = Rc::downgrade(&rc);
        let client: C = new_client_from_rc(rc);
        self.caps.insert(client.as_client_hook().get_ptr(), weak);
        client
    }

    /// Adds a server with an independent owner for non-cancellable local calls.
    pub fn new_client_with_executor(
        &mut self,
        server: S,
        executor: Rc<dyn capnp::capability::CallExecutor>,
    ) -> C {
        self.new_client_from_rc_with_executor(Rc::new(server), executor)
    }

    /// Variant of [`Self::new_client_with_executor`] for a shared server.
    pub fn new_client_from_rc_with_executor(
        &mut self,
        server: Rc<S>,
        executor: Rc<dyn capnp::capability::CallExecutor>,
    ) -> C {
        let weak = Rc::downgrade(&server);
        let client: C = new_client_from_rc_with_executor(server, executor);
        self.caps.insert(client.as_client_hook().get_ptr(), weak);
        client
    }

    /// Looks up a capability and returns its underlying server object, if found.
    /// Follows available resolutions, recognizing local members before waiting
    /// for further shortening. Waits behind streaming calls already queued on a
    /// member; later calls do not extend that lookup barrier.
    pub async fn get_local_server(&self, client: &C) -> Option<Rc<S>>
    where
        C: capnp::capability::FromClientHook,
    {
        let mut hook = client.as_client_hook().add_ref();
        loop {
            while let Some(resolved) = hook.get_resolved() {
                hook = resolved;
            }
            let ptr = hook.get_ptr();
            if let Some(server) = self.caps.get(&ptr).filter(|server| {
                // A stale set entry must never recognize a reused client address.
                local::Client::<C::Dispatch>::is_registered(server.as_ptr() as usize, ptr)
            }) {
                hook.when_local_server_ready().await.ok()?;
                return server.upgrade().filter(|server| {
                    local::Client::<C::Dispatch>::is_registered(Rc::as_ptr(server) as usize, ptr)
                });
            }
            hook = hook.when_more_resolved()?.await.ok()?;
        }
    }

    /// Looks up a capability and returns its underlying server object, if found.
    /// Follows already available resolutions without waiting for a promise.
    /// The advantage over `get_local_server()` is that it borrows `self`
    /// over a shorter span (which can be very important if `self` is inside a `RefCell`).
    /// Returns `None` while the local streaming queue is blocked.
    pub fn get_local_server_of_resolved(&self, client: &C) -> Option<Rc<S>>
    where
        C: capnp::capability::FromClientHook,
    {
        let mut hook = client.as_client_hook().add_ref();
        while let Some(resolved) = hook.get_resolved() {
            hook = resolved;
        }
        if !hook.is_local_server_ready() {
            return None;
        }
        let ptr = hook.get_ptr();
        let server = self.caps.get(&ptr)?.upgrade()?;
        local::Client::<C::Dispatch>::is_registered(Rc::as_ptr(&server) as usize, ptr)
            .then_some(server)
    }

    /// Reclaim memory used for Server objects that no longer exist.
    pub fn gc(&mut self) {
        self.caps.retain(|ptr, server| {
            local::Client::<C::Dispatch>::is_registered(server.as_ptr() as usize, *ptr)
        });
    }
}

/// Creates a `Client` from a future that resolves to a `Client`.
///
/// Any calls that arrive before the resolution are accumulated in a queue.
#[cfg(feature = "std")]
pub fn new_future_client<T>(
    client_future: impl ::futures::Future<Output = Result<T, Error>> + 'static,
) -> T
where
    T: ::capnp::capability::FromClientHook,
{
    let mut queued_client = crate::queued::Client::new(None);
    let weak_client = Rc::downgrade(&queued_client.inner);

    queued_client.drive(client_future.then(move |r| {
        if let Some(queued_inner) = weak_client.upgrade() {
            crate::queued::ClientInner::resolve(&queued_inner, r.map(|c| c.into_client_hook()));
        }
        Promise::ok(())
    }));

    T::new(Box::new(queued_client))
}

#[cfg(feature = "std")]
struct SystemTaskReaper;
#[cfg(feature = "std")]
impl crate::task_set::TaskReaper<Error> for SystemTaskReaper {
    fn task_failed(&mut self, error: Error) {
        println!("ERROR: {error}");
    }
}

#[cfg(feature = "std")]
pub struct ImbuedMessageBuilder<A>
where
    A: ::capnp::message::Allocator,
{
    builder: ::capnp::message::Builder<A>,
    cap_table: Vec<Option<Box<dyn ::capnp::private::capability::ClientHook>>>,
}

#[cfg(feature = "std")]
impl<A> ImbuedMessageBuilder<A>
where
    A: ::capnp::message::Allocator,
{
    pub fn new(allocator: A) -> Self {
        Self {
            builder: ::capnp::message::Builder::new(allocator),
            cap_table: Vec::new(),
        }
    }

    pub fn get_root<'a, T>(&'a mut self) -> ::capnp::Result<T>
    where
        T: ::capnp::traits::FromPointerBuilder<'a>,
    {
        use capnp::traits::ImbueMut;
        let mut root: ::capnp::any_pointer::Builder = self.builder.get_root()?;
        root.imbue_mut(&mut self.cap_table);
        root.get_as()
    }

    pub fn set_root<T: ::capnp::traits::Owned>(
        &mut self,
        value: impl ::capnp::traits::SetterInput<T>,
    ) -> ::capnp::Result<()> {
        use capnp::traits::ImbueMut;
        let mut root: ::capnp::any_pointer::Builder = self.builder.get_root()?;
        root.imbue_mut(&mut self.cap_table);
        root.set_as(value)
    }
}

#[cfg(feature = "std")]
fn canceled_to_error(_e: capntproto_rpc_engine::completion::Error) -> Error {
    Error::failed("oneshot was canceled".to_string())
}

/// Standard typed Persistent(SturdyRef, Owner) interface from the pinned C++ schema.
#[allow(clippy::extra_unused_type_parameters)] // Generated generic schema helpers.
#[cfg(feature = "std")]
pub mod persistent_capnp {
    include!(concat!(env!("OUT_DIR"), "/persistent_capnp.rs"));
}
