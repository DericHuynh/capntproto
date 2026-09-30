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
//!     async fn identity(
//!         self: Rc<Self>,
//!         params: foo::IdentityParams,
//!         mut results: foo::IdentityResults
//!     ) -> Result<(), ::capnp::Error> {
//!         let x = params.get()?.get_x();
//!         results.get().set_y(x);
//!         Ok(())
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

use capnp::capability::Promise;
use capnp::private::capability::ClientHook;
use capnp::Error;
use futures::channel::oneshot;
use futures::{Future, FutureExt};
use std::cell::RefCell;
use std::pin::Pin;
use std::rc::{Rc, Weak};
use std::task::{Context, Poll};

pub use crate::rpc::join::Joiner;
pub use crate::rpc::Disconnector;
use crate::task_set::TaskSet;

pub use crate::reconnect::{auto_reconnect, lazy_auto_reconnect, SetTarget};
mod pipeline_builder;
pub use pipeline_builder::PipelineBuilder;

/// Code generated from
/// [rpc.capnp](https://github.com/capnproto/capnproto/blob/master/c%2B%2B/src/capnp/rpc.capnp).
#[allow(clippy::extra_unused_type_parameters)] // Generated generic schema helpers.
pub mod rpc_capnp {
    include!(concat!(env!("OUT_DIR"), "/rpc_capnp.rs"));
}

/// Code generated from
/// [rpc-twoparty.capnp](https://github.com/capnproto/capnproto/blob/master/c%2B%2B/src/capnp/rpc-twoparty.capnp).
#[allow(clippy::extra_unused_type_parameters)] // Generated generic schema helpers.
pub mod rpc_twoparty_capnp {
    include!(concat!(env!("OUT_DIR"), "/rpc_twoparty_capnp.rs"));
}

/// Like [`try!()`], but for functions that return a [`Promise<T, E>`] rather than a [`Result<T, E>`].
///
/// Unwraps a `Result<T, E>`. In the case of an error `Err(e)`, immediately returns from the
/// enclosing function with `Promise::err(e)`.
#[macro_export]
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

mod attach;
mod broken;
mod fd;
mod flow_control;
mod local;
pub mod membrane;
pub mod multiparty;
mod queued;
mod reconnect;
mod revocable;
pub use revocable::RevocableServer;
mod rpc;
mod sender_queue;
mod split;
mod task_set;
pub mod third_party;
pub mod twoparty;

use capnp::message;

/// A message to be sent by a [`VatNetwork`].
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

    /// Sends the message. Returns a promise that resolves once the send has completed.
    /// Dropping the returned promise does *not* cancel the send.
    fn send(
        self: Box<Self>,
    ) -> (
        Promise<(), Error>,
        Rc<message::Builder<message::HeapAllocator>>,
    );

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
pub fn is_short_lived_rpc_message(body: capnp::any_pointer::Reader<'_>) -> capnp::Result<bool> {
    use crate::rpc_capnp::message;
    Ok(!matches!(
        body.get_as::<message::Reader>()?.which(),
        Ok(message::Call(_) | message::Return(_))
    ))
}

/// A message received from a [`VatNetwork`].
pub trait IncomingMessage {
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

/// A two-way RPC connection.
///
/// A connection can be created by [`VatNetwork::connect()`].
pub trait Connection<VatId> {
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

    /// Waits for a message to be received and returns it.  If the read stream cleanly terminates,
    /// returns None. If any other problem occurs, returns an Error.
    fn receive_incoming_message(&mut self) -> Promise<Option<Box<dyn IncomingMessage>>, Error>;

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
pub fn new_adaptive_flow_controller_with_clock(
    initial_window: usize,
    clock: impl Fn() -> std::time::Duration + 'static,
) -> (Box<dyn FlowController>, Promise<(), Error>) {
    let (controller, driver) =
        crate::flow_control::WindowFlowController::new(initial_window, None, Some(Rc::new(clock)));
    (Box::new(controller), driver)
}

/// Tracks a particular RPC stream in order to implement a flow control algorithm.
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
pub trait VatNetwork<VatId> {
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
pub trait BootstrapFactory<VatId> {
    fn create_for(&self, client_id: &VatId) -> capnp::Result<capnp::capability::Client>;
}

pub(crate) enum Bootstrap<VatId> {
    Static(Box<dyn ClientHook>),
    Factory {
        factory: Rc<dyn BootstrapFactory<VatId>>,
        local_id: Rc<VatId>,
    },
}
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
pub struct RpcSystem<VatId>
where
    VatId: 'static,
{
    network: Rc<RefCell<Box<dyn crate::VatNetwork<VatId>>>>,

    bootstrap: Bootstrap<VatId>,

    connections: Rc<RefCell<rpc::ConnectionRegistry<VatId>>>,
    join_context: Rc<rpc::JoinContext<VatId>>,

    tasks: TaskSet<Error>,
    handle: crate::task_set::TaskSetHandle<Error>,
}

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
        let (mut handle, tasks) = TaskSet::new(Box::new(SystemTaskReaper));

        let mut handle1 = handle.clone();
        handle.add(network.drive_until_shutdown().then(move |r| {
            let r = match r {
                Ok(()) => Ok(()),
                Err(e) => {
                    if e.kind != ::capnp::ErrorKind::Disconnected {
                        // Don't report disconnects as an error.
                        Err(e)
                    } else {
                        Ok(())
                    }
                }
            };

            handle1.terminate(r);
            Promise::ok(())
        }));

        let join_network = network.join_network();
        let join_context = Rc::new(rpc::JoinContext {
            bootstrap: bootstrap.clone(),
            tasks: handle.clone(),
        });
        let mut result = Self {
            join_context,
            network: Rc::new(RefCell::new(network)),
            bootstrap,
            connections: Rc::new(RefCell::new(rpc::ConnectionRegistry::new())),

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
        if self.connections.borrow().closing {
            return T::new(broken::new_cap(Error::disconnected(
                "RPC system is closing".into(),
            )));
        }
        let connection = self.network.borrow_mut().connect(vat_id);
        let Some(connection) = connection else {
            return T::new(self.bootstrap.local().unwrap_or_else(broken::new_cap));
        };
        let connection_state = Self::get_connection_state(
            &self.connections,
            self.bootstrap.clone(),
            connection,
            self.handle.clone(),
        );

        let hook = rpc::ConnectionState::bootstrap(&connection_state);
        T::new(hook)
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
                if connections.borrow().closing {
                    return Ok(());
                }
                Self::get_connection_state(
                    &connections,
                    bootstrap.clone(),
                    connection,
                    handle.clone(),
                );
            }
        })
    }

    pub(crate) fn get_connection_state(
        connections: &Rc<RefCell<rpc::ConnectionRegistry<VatId>>>,
        bootstrap: Bootstrap<VatId>,
        connection: Box<dyn crate::Connection<VatId>>,
        mut handle: crate::task_set::TaskSetHandle<Error>,
    ) -> Rc<rpc::ConnectionState<VatId>> {
        let id = connection.connection_id();
        let existing = connections.borrow().states.get(&id).cloned();
        if let Some(state) = existing {
            state.set_not_idle();
            return state;
        }
        let (sender, receiver) = oneshot::channel::<Promise<(), Error>>();
        let (tasks, state) = rpc::ConnectionState::new(
            bootstrap,
            connection,
            sender,
            Rc::downgrade(connections),
            handle.clone(),
            connections.borrow().flow_limit,
        );
        connections.borrow_mut().states.insert(id, state.clone());
        let registry = connections.clone();
        let shutdown_state = state.clone();
        handle.add(async move {
            let result = match receiver.await {
                Ok(shutdown) => shutdown.await,
                Err(e) => Err(Error::failed(e.to_string())),
            };
            let waker = {
                let mut registry = registry.borrow_mut();
                // Live entries are removed before shutdown. The disconnector
                // still waits for all outstanding flushes.
                registry.closing_connections -= 1;
                registry.waker.take()
            };
            drop(shutdown_state);
            if let Some(waker) = waker {
                waker.wake();
            }
            result
        });
        handle.add(tasks);
        state
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
            registry.states.values().cloned().collect::<Vec<_>>()
        };
        for state in states {
            state.set_flow_limit(words);
        }
    }

    /// Returns a `Disconnector` future that can be run to cleanly close the connection to this `RpcSystem`'s network.
    /// You should get the `Disconnector` before you spawn the `RpcSystem`.
    pub fn get_disconnector(&self) -> rpc::Disconnector<VatId> {
        rpc::Disconnector::new(self.connections.clone())
    }
}

impl<VatId: 'static> Drop for RpcSystem<VatId> {
    fn drop(&mut self) {
        let states = {
            let mut registry = self.connections.borrow_mut();
            registry.closing = true;
            registry.states.values().cloned().collect::<Vec<_>>()
        };
        for state in states {
            state.disconnect(Error::disconnected("RPC system was dropped".into()));
        }
    }
}

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

pub(crate) struct CallTaskExecutor(task_set::TaskSetHandle<Error>);
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
pub fn new_client<C, S>(s: S) -> C
where
    C: capnp::capability::FromServer<S>,
{
    new_client_from_rc(Rc::new(s))
}

/// Construct a server from an owned runtime schema. Protected direct calls
/// require new_loaded_client_from_rc() with an executor.
pub fn new_loaded_client<S: capnp::schema_loader::dynamic::Server>(
    server: S,
    schema: capnp::schema_loader::dynamic::ServiceSchema,
) -> capnp::capability::Client {
    new_loaded_client_from_rc(Rc::new(server), schema, None)
}

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
pub fn new_dynamic_client<S: capnp::dynamic_capability::Server>(
    server: S,
) -> capnp::dynamic_capability::Client {
    new_dynamic_client_from_rc(Rc::new(server), None)
}

/// Shared-server variant with an optional owner for protected local calls.
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
pub struct CapabilityServerSet<S, C>
where
    C: capnp::capability::FromServer<S>,
{
    caps: std::collections::HashMap<usize, Weak<S>>,
    marker: std::marker::PhantomData<C>,
}

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

struct SystemTaskReaper;
impl crate::task_set::TaskReaper<Error> for SystemTaskReaper {
    fn task_failed(&mut self, error: Error) {
        println!("ERROR: {error}");
    }
}

pub struct ImbuedMessageBuilder<A>
where
    A: ::capnp::message::Allocator,
{
    builder: ::capnp::message::Builder<A>,
    cap_table: Vec<Option<Box<dyn ::capnp::private::capability::ClientHook>>>,
}

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

fn canceled_to_error(_e: futures::channel::oneshot::Canceled) -> Error {
    Error::failed("oneshot was canceled".to_string())
}

/// Standard typed Persistent(SturdyRef, Owner) interface from the pinned C++ schema.
#[allow(clippy::extra_unused_type_parameters)] // Generated generic schema helpers.
pub mod persistent_capnp {
    include!(concat!(env!("OUT_DIR"), "/persistent_capnp.rs"));
}
