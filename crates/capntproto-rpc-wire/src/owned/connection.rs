//! Owning coordination of incoming messages, ordered calls and outgoing replies.
//! Storage and scheduling remain with the caller. Capacity pressure retains an
//! admitted operation, so retry never receives its capability grants twice.
use super::*;
use crate::clients::{self as binding, outgoing, EndpointOwner};
use capntproto_rpc_engine::{
    clients::{self, dispatch},
    connection, questions,
    storage::Storage as SlotStorage,
};

pub type Answer<'pool, 'id, A, const CAPS: usize> =
    answer::State<'pool, pipelines::Results<'pool, 'id, A, CAPS>>;
pub type Request<'pool, 'id, I, const CAPS: usize, const PATH: usize> =
    Call<'pool, 'id, 'id, 'id, I, CAPS, PATH>;
pub type Response<'pool, 'id, I, const CAPS: usize> =
    super::Response<'pool, 'id, 'id, 'id, 'id, I, CAPS>;
pub type Runtime<'pool, 'id, 's, I, P, S, const CAPS: usize, const PATH: usize> = dispatch::Runtime<
    'id,
    's,
    's,
    binding::Route<'id, P, S, PATH>,
    P,
    Task<'pool, 'id, I, CAPS, PATH>,
>;

pub enum Task<'pool, 'id, I, const CAPS: usize, const PATH: usize> {
    Call(Request<'pool, 'id, I, CAPS, PATH>),
    Invoke(Invocation<'pool, 'id, CAPS>),
    Loopback(super::loopback::Ticket<'id>),
    Echo(super::loopback::Echo<'id, PATH>),
    ProvisionBarrier(QueuedProvisionBarrier<'pool, 'id>),
    VineBarrier(QueuedVineBarrier<'pool, 'id>),
}
impl<'pool, 'id, I, const CAPS: usize, const PATH: usize> Task<'pool, 'id, I, CAPS, PATH> {
    pub fn as_call(&self) -> Option<&Request<'pool, 'id, I, CAPS, PATH>> {
        match self {
            Self::Call(call) => Some(call),
            _ => None,
        }
    }
    // Rejection must return the original affine owner without requiring alloc.
    #[allow(clippy::result_large_err)]
    pub fn into_call(self) -> Result<Request<'pool, 'id, I, CAPS, PATH>, Self> {
        match self {
            Self::Call(call) => Ok(call),
            other => Err(other),
        }
    }
}

/// Optional allocation-backed reply handoff. Deferred frames are untouched;
/// Delivered replies have released their original question/message owners.
#[cfg(feature = "alloc")]
pub enum PlainReceived<'pool, T> {
    Deferred(Frame<'pool>),
    Delivered(T, alloc::boxed::Box<super::PlainResults>),
}

#[cfg(feature = "alloc")]
mod plain_return;

enum Exchange<'pool, 'id, I, const CAPS: usize> {
    Pending(Question<'pool, 'id>),
    Returned(Response<'pool, 'id, I, CAPS>),
}
impl<'id, I, const CAPS: usize> Exchange<'_, 'id, I, CAPS> {
    fn key(&self) -> questions::Key<'id> {
        match self {
            Self::Pending(question) => question.key(),
            Self::Returned(response) => response.key(),
        }
    }
}
pub struct ExchangeSlot<'pool, 'id, I, const CAPS: usize> {
    exchange: Option<Exchange<'pool, 'id, I, CAPS>>,
    authorized: bool,
    canceled: bool,
    tail_taken: bool,
    retain_handoff_routes: bool,
    handoff: Option<Result<LocalResult<'pool, 'id, CAPS>, capnp::ErrorKind>>,
}
impl<I, const CAPS: usize> ExchangeSlot<'_, '_, I, CAPS> {
    pub const EMPTY: Self = Self {
        exchange: None,
        authorized: false,
        canceled: false,
        tail_taken: false,
        retain_handoff_routes: false,
        handoff: None,
    };
}

pub struct Storage<'pool, 'id, 's, I, const CAPS: usize, const PATH: usize> {
    /// At least two slots. One is reserved for a sender loopback so application
    /// calls waiting on its echo cannot consume the capacity needed to send it.
    pub tasks: &'s mut [dispatch::TaskSlot<Task<'pool, 'id, I, CAPS, PATH>>],
    pub imports: &'s mut [binding::ImportSlot<'id, 'id>],
    pub pipelines: &'s mut [pipelines::Slot<'id, 'id, PATH>],
    pub outgoing: &'s mut [outgoing::Slot<'id, PATH>],
    pub exported: &'s mut [binding::exporting::Slot<'id, 'id>],
    pub loopbacks: &'s mut [super::loopback::Slot<'id>],
    pub embargoes: &'s mut [super::loopback::ReceiptSlot<'id>],
    pub exchanges: &'s mut [ExchangeSlot<'pool, 'id, I, CAPS>],
    pub tails: &'s mut [TailSlot<'id>],
    pub invocation_pipelines: &'s mut [InvocationPipelineSlot<'id, PATH>],
}

struct PreparedStorage<'pool, 'id, 's, I, const CAPS: usize, const PATH: usize> {
    /// At least two slots. One is reserved for a sender loopback so application
    /// calls waiting on its echo cannot consume the capacity needed to send it.
    message_capabilities: Capacity,
    message_path: Capacity,
    tasks: SlotStorage<'s, dispatch::TaskSlot<Task<'pool, 'id, I, CAPS, PATH>>>,
    imports: SlotStorage<'s, binding::ImportSlot<'id, 'id>>,
    pipelines: SlotStorage<'s, pipelines::Slot<'id, 'id, PATH>>,
    outgoing: SlotStorage<'s, outgoing::Slot<'id, PATH>>,
    exported: SlotStorage<'s, binding::exporting::Slot<'id, 'id>>,
    loopbacks: SlotStorage<'s, super::loopback::Slot<'id>>,
    embargoes: SlotStorage<'s, super::loopback::ReceiptSlot<'id>>,
    exchanges: SlotStorage<'s, ExchangeSlot<'pool, 'id, I, CAPS>>,
    tails: SlotStorage<'s, TailSlot<'id>>,
    invocation_pipelines: SlotStorage<'s, InvocationPipelineSlot<'id, PATH>>,
}

/// A connection retains every original question through its late Return, even
/// after application cancellation. Ordinary input controls may progress while
/// a Call or Resolve awaits graph/dispatch capacity. The caller must eventually
/// service retained operations or terminate the connection on exhaustion.
pub struct Driver<'pool, 'id, 's, I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize> {
    allow_answer_handoff: bool,
    defer_calls: bool,
    owned_output: bool,
    // Conservative until the ordinary canceled-response scan proves absence.
    // Exchange capacity alone does not make pending questions pollable.
    canceled_exchanges: bool,
    descriptors: crate::publication::descriptors::Descriptors<binding::Route<'id, P, S, PATH>>,
    pub ledgers:
        connection::Connection<'id, 's, I, clients::Client<'id>, Answer<'pool, 'id, A, CAPS>>,
    pub runtime: Runtime<'pool, 'id, 's, I, P, S, CAPS, PATH>,
    pub imports: binding::Imports<'s, 'id, 'id, P>,
    pub pipelines: pipelines::Bindings<'s, 'id, 'id, PATH>,
    pub exported: binding::exporting::Bindings<'s, 'id, 'id>,
    outgoing: outgoing::Bindings<'s, 'id, P, PATH>,
    exchanges: SlotStorage<'s, ExchangeSlot<'pool, 'id, I, CAPS>>,
    call: Option<Request<'pool, 'id, I, CAPS, PATH>>,
    resolution: Option<Resolution<'pool, 'id, 'id, 'id, I>>,
    loopbacks: super::loopback::Bindings<'s, 'id>,
    barrier: Option<super::loopback::Ticket<'id>>,
    echo: Option<super::loopback::Echo<'id, PATH>>,
    network: Option<NetworkRequest<'pool, 'id, PATH>>,
    disconnected: Option<capnp::ErrorKind>,
    tails: SlotStorage<'s, TailSlot<'id>>,
    invocation_pipelines: SlotStorage<'s, InvocationPipelineSlot<'id, PATH>>,
    message_capabilities: Capacity,
    message_path: Capacity,
    maintenance_sizes: [usize; 18],
    maintenance_total: usize,
    maintenance_phase: usize,
    maintenance_index: usize,
    scan_remaining: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    NonemptyStorage,
    InsufficientControlStorage,
    Graph(clients::Error),
    Imports(binding::Error),
    Pipeline(pipelines::Error),
    Outgoing(outgoing::Error),
    Question(questions::Error),
    Exported(binding::exporting::Error),
    Loopback(super::loopback::Error),
    Dispatch(dispatch::DispatchError),
    InvalidLoopbackTarget,
    InvalidNetworkTarget,
    Disconnected(capnp::ErrorKind),
    Input(super::Error),
    Enqueue(EnqueueError),
    Resolve(ApplyError),
    Busy,
    UnknownQuestion,
    PendingResponse,
    PendingAuthorization,
    RedirectedResponse,
    TailAlreadyUsed,
    InvalidInvocationTarget,
    UnknownInvocation,
}

/// One bounded maintenance result. Yield asks the caller to schedule another
/// poll; Idle means a full scan found no runnable maintenance. IO and application
/// tasks may still be pending. Owners are detached before being returned here.
pub enum Progress<I, A, S> {
    Idle,
    Yield,
    Advanced,
    Import(I),
    Producer(A),
    Server(S),
}
/// Promise propagation keeps any loopback obligation inside this driver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Propagation {
    Unrelated,
    Pending,
    Abandoned,
    Resolved,
}
impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "RPC connection: {self:?}")
    }
}
impl core::error::Error for Error {}

pub enum Received<'id, A> {
    Queued(answers::Key<'id>),
    DeferredCall(answers::Key<'id>),
    Bootstrap(Bootstrap<'id>),
    Returned(questions::Key<'id>),
    Resolved,
    DeferredResolution,
    Finished(Option<A>),
    Released,
    Loopback,
    /// Take the owning request and complete it through the authenticated network.
    Network,
    /// An ordered network marker must first pass earlier dispatch work.
    QueuedNetwork,
    Unimplemented,
    /// Stop transport IO and consume this driver with `terminate` for cleanup.
    Aborted(capnp::ErrorKind),
}
#[must_use = "retry or release the unadmitted input and its producer"]
pub struct Rejected<'pool, A> {
    pub error: Error,
    pub frame: Frame<'pool>,
    pub producer: Option<A>,
}

enum IncomingKind {
    Call(usize),
    Bootstrap,
    Provide,
    Accept,
    ThirdPartyAnswer,
    Join,
    Return(u32, bool),
    Resolve,
    Finish(AnswerId, bool),
    Release,
    SenderLoopback,
    ReceiverLoopback(u32),
    ProvisionBarrier,
    Abort(capnp::ErrorKind),
    Unimplemented(Option<ExportId>),
}

impl<'pool, 'id, 's, I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize>
    Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>
{
    /// Observe allocated loopback IDs, including output offers awaiting acceptance.
    #[cfg(feature = "alloc")]
    pub fn observe_embargoes(&mut self) -> capntproto_rpc_engine::observation::Observation {
        self.loopbacks.observe_receipts()
    }
    /// No protocol references or deferred work remain. The transport owner must
    /// additionally drain application work and queued/flushing output before
    /// advertising idle. A scheduler's Pending alone does not imply idle.
    pub fn is_idle(&self) -> bool {
        self.disconnected.is_none()
            && self.ledgers.questions.is_empty()
            && self.ledgers.answers.is_empty()
            && self.ledgers.imports.is_empty()
            && self.ledgers.exports.is_empty()
            && self.runtime.is_empty()
            && self.loopbacks.is_empty()
            && self.call.is_none()
            && self.resolution.is_none()
            && self.barrier.is_none()
            && self.echo.is_none()
            && self.network.is_none()
            && self
                .exchanges
                .iter()
                .all(|slot| slot.exchange.is_none() && slot.handoff.is_none())
            && self.tails.iter().all(|slot| !slot.is_occupied())
    }
    #[allow(clippy::type_complexity, clippy::result_large_err)]
    pub fn new(
        connection: connection::GraphConnection<
            'id,
            's,
            I,
            Answer<'pool, 'id, A, CAPS>,
            binding::Route<'id, P, S, PATH>,
            P,
        >,
        storage: Storage<'pool, 'id, 's, I, CAPS, PATH>,
        peer: P,
    ) -> Result<
        Self,
        (
            Error,
            connection::GraphConnection<
                'id,
                's,
                I,
                Answer<'pool, 'id, A, CAPS>,
                binding::Route<'id, P, S, PATH>,
                P,
            >,
        ),
    > {
        Self::from_storage(
            connection,
            PreparedStorage {
                tasks: SlotStorage::borrowed(storage.tasks),
                imports: SlotStorage::borrowed(storage.imports),
                pipelines: SlotStorage::borrowed(storage.pipelines),
                outgoing: SlotStorage::borrowed(storage.outgoing),
                exported: SlotStorage::borrowed(storage.exported),
                loopbacks: SlotStorage::borrowed(storage.loopbacks),
                embargoes: SlotStorage::borrowed(storage.embargoes),
                exchanges: SlotStorage::borrowed(storage.exchanges),
                tails: SlotStorage::borrowed(storage.tails),
                invocation_pipelines: SlotStorage::borrowed(storage.invocation_pipelines),
                message_capabilities: Capacity::Inline,
                message_path: Capacity::Inline,
            },
            peer,
        )
    }

    #[allow(clippy::type_complexity, clippy::result_large_err)]
    fn from_storage(
        connection: connection::GraphConnection<
            'id,
            's,
            I,
            Answer<'pool, 'id, A, CAPS>,
            binding::Route<'id, P, S, PATH>,
            P,
        >,
        storage: PreparedStorage<'pool, 'id, 's, I, CAPS, PATH>,
        peer: P,
    ) -> Result<
        Self,
        (
            Error,
            connection::GraphConnection<
                'id,
                's,
                I,
                Answer<'pool, 'id, A, CAPS>,
                binding::Route<'id, P, S, PATH>,
                P,
            >,
        ),
    > {
        if storage.tasks.maximum() < 2
            || storage.loopbacks.maximum() == 0
            || storage.embargoes.maximum() == 0
        {
            return Err((Error::InsufficientControlStorage, connection));
        }
        if storage.exchanges.iter().any(|slot| slot.exchange.is_some())
            || storage.tails.iter().any(|slot| slot.is_occupied())
            || storage
                .invocation_pipelines
                .iter()
                .any(|slot| slot.is_occupied())
        {
            return Err((Error::NonemptyStorage, connection));
        }
        let maintenance_sizes = [
            // The first fifteen phases retain their existing cursor order.
            1,
            1,
            storage.pipelines.len().max(1),
            storage.outgoing.len().max(1),
            connection.ledgers.exports.capacity().max(1),
            1,
            connection.ledgers.answers.capacity().max(1),
            1,
            storage.exported.len().max(1),
            1,
            connection.ledgers.imports.capacity().max(1),
            1,
            1,
            1,
            1,
            storage.tails.len().max(1),
            storage.invocation_pipelines.len().max(1),
            1,
        ];
        let Some(scan_remaining) = maintenance_sizes
            .iter()
            .try_fold(0usize, |total, size| total.checked_add(*size))
        else {
            return Err((Error::Input(super::Error::Capacity), connection));
        };
        let exported = match binding::exporting::Bindings::from_storage(storage.exported) {
            Ok(value) => value,
            Err(error) => return Err((Error::Exported(error), connection)),
        };
        let loopbacks =
            match super::loopback::Bindings::from_storage(storage.loopbacks, storage.embargoes) {
                Ok(value) => value,
                Err(error) => return Err((Error::Loopback(error), connection)),
            };
        let imports = match binding::Imports::from_storage(storage.imports, peer) {
            Ok(value) => value,
            Err(error) => return Err((Error::Imports(error), connection)),
        };
        let pipelines = match pipelines::Bindings::from_storage_with_capacity(
            storage.pipelines,
            storage.message_path,
        ) {
            Ok(value) => value,
            Err(error) => return Err((Error::Pipeline(error), connection)),
        };
        let outgoing = match outgoing::Bindings::from_storage_with_capacity(
            storage.outgoing,
            peer,
            storage.message_path,
        ) {
            Ok(value) => value,
            Err(error) => return Err((Error::Outgoing(error), connection)),
        };
        let runtime = match dispatch::own_runtime_with_storage(connection.clients, storage.tasks) {
            Ok(value) => value,
            Err(rejected) => {
                return Err((
                    Error::Graph(rejected.error),
                    connection::GraphConnection {
                        ledgers: connection.ledgers,
                        clients: rejected.value,
                    },
                ))
            }
        };
        Ok(Self {
            allow_answer_handoff: false,
            defer_calls: false,
            owned_output: false,
            canceled_exchanges: false,
            descriptors: Default::default(),
            ledgers: connection.ledgers,
            message_capabilities: storage.message_capabilities,
            message_path: storage.message_path,
            runtime,
            imports,
            pipelines,
            exported,
            outgoing,
            exchanges: storage.exchanges,
            call: None,
            resolution: None,
            loopbacks,
            barrier: None,
            echo: None,
            network: None,
            disconnected: None,
            tails: storage.tails,
            invocation_pipelines: storage.invocation_pipelines,
            maintenance_sizes,
            maintenance_total: scan_remaining,
            maintenance_phase: 0,
            maintenance_index: 0,
            scan_remaining,
        })
    }

    /// Take responsibility for the original request immediately after its FIFO
    /// admission. Failure returns the request; it must still be tracked through
    /// Return or retained by the caller until terminal teardown.
    pub fn track(
        &mut self,
        question: Question<'pool, 'id>,
    ) -> Result<questions::Key<'id>, Question<'pool, 'id>> {
        let Some(slot) = self
            .exchanges
            .vacant_mut(|_, slot| slot.exchange.is_none(), || ExchangeSlot::EMPTY)
        else {
            return Err(question);
        };
        let key = question.key();
        slot.exchange = Some(Exchange::Pending(question));
        slot.authorized = true;
        slot.canceled = false;
        slot.tail_taken = false;
        Ok(key)
    }

    /// Reserve request tracking before building and publishing an ordinary
    /// outgoing call or bootstrap. Once FIFO admission succeeds, its original
    /// request is already owned by this driver through the matching Return.
    pub fn send(
        &mut self,
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, impl Default>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        build: impl FnOnce(
            &mut capnp::message::FixedBuilder<'_>,
            capntproto_rpc_engine::ids::QuestionId,
        ) -> capnp::Result<()>,
    ) -> Result<questions::Key<'id>, Error> {
        self.send_with(
            questions::ResultsTo::Caller,
            messages,
            queue,
            minimum,
            options,
            build,
        )
    }
    pub fn send_redirected(
        &mut self,
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, impl Default>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        build: impl FnOnce(
            &mut capnp::message::FixedBuilder<'_>,
            capntproto_rpc_engine::ids::QuestionId,
        ) -> capnp::Result<()>,
    ) -> Result<questions::Key<'id>, Error> {
        self.send_with(
            questions::ResultsTo::Yourself,
            messages,
            queue,
            minimum,
            options,
            build,
        )
    }
    /// Send a call whose result will be delivered on an authenticated third-party
    /// connection. It cannot be used as a same-connection tail adoption source.
    pub fn send_third_party(
        &mut self,
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, impl Default>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        build: impl FnOnce(
            &mut capnp::message::FixedBuilder<'_>,
            capntproto_rpc_engine::ids::QuestionId,
        ) -> capnp::Result<()>,
    ) -> Result<questions::Key<'id>, Error> {
        self.send_with(
            questions::ResultsTo::ThirdParty,
            messages,
            queue,
            minimum,
            options,
            build,
        )
    }
    fn send_with(
        &mut self,
        destination: questions::ResultsTo,
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, impl Default>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        build: impl FnOnce(
            &mut capnp::message::FixedBuilder<'_>,
            capntproto_rpc_engine::ids::QuestionId,
        ) -> capnp::Result<()>,
    ) -> Result<questions::Key<'id>, Error> {
        self.check_live()?;
        let slot = self
            .exchanges
            .vacant_mut(|_, slot| slot.exchange.is_none(), || ExchangeSlot::EMPTY)
            .ok_or(Error::Busy)?;
        let output = queue
            .reserve()
            .map_err(|error| Error::Input(super::Error::Storage(error)))?;
        let reservation = match destination {
            questions::ResultsTo::Caller => self.ledgers.questions.reserve(),
            questions::ResultsTo::Yourself => self.ledgers.questions.reserve_redirected(),
            questions::ResultsTo::ThirdParty => self.ledgers.questions.reserve_third_party(),
        }
        .map_err(Error::Question)?;
        let frame = Frame::build(messages, minimum, options, |builder| {
            build(builder, reservation.id())
        })
        .map_err(Error::Input)?;
        let question = Question::send(reservation, messages, frame, output)
            .map_err(|error| Error::Input(super::Error::Output(error)))?;
        let key = question.key();
        slot.exchange = Some(Exchange::Pending(question));
        slot.authorized = true;
        slot.canceled = false;
        slot.tail_taken = false;
        Ok(key)
    }

    pub fn pipeline(
        &mut self,
        key: questions::Key<'id>,
        path: pipeline::Path<'_>,
    ) -> Result<clients::Client<'id>, Error> {
        self.check_live()?;
        let slot = self
            .exchanges
            .iter()
            .find(|slot| {
                slot.exchange
                    .as_ref()
                    .is_some_and(|exchange| exchange.key() == key)
            })
            .ok_or(Error::UnknownQuestion)?;
        if !slot.authorized {
            return Err(Error::PendingAuthorization);
        }
        match slot.exchange.as_ref().unwrap() {
            Exchange::Pending(question) => self.outgoing.bind(
                question,
                &mut self.ledgers.questions,
                &mut self.runtime,
                path,
            ),
            Exchange::Returned(response) => {
                self.outgoing
                    .observe_returned(response, &mut self.runtime, path)
            }
        }
        .map_err(Error::Outgoing)
    }

    pub fn cancel(&mut self, key: questions::Key<'id>) -> Result<(), Error> {
        let pending_invocation = self
            .invocation_pipelines
            .iter()
            .any(|slot| slot.pending_for(key));
        let slot = self
            .exchanges
            .iter_mut()
            .find(|slot| {
                slot.exchange
                    .as_ref()
                    .is_some_and(|exchange| exchange.key() == key)
            })
            .ok_or(Error::UnknownQuestion)?;
        if let Some(Exchange::Pending(question)) = &mut slot.exchange {
            if !pending_invocation {
                question.cancel(&mut self.ledgers.questions);
            }
        }
        slot.canceled = true;
        self.canceled_exchanges = true;
        Ok(())
    }

    /// A Return stays retained until every preexisting pipeline observer has
    /// consumed it, including when the application canceled its result.
    pub fn take_response(
        &mut self,
        key: questions::Key<'id>,
    ) -> Result<Response<'pool, 'id, I, CAPS>, Error> {
        if self.outgoing.pending_for(key)
            || self
                .invocation_pipelines
                .iter()
                .any(|slot| slot.pending_for(key))
        {
            return Err(Error::PendingResponse);
        }
        let slot = self
            .exchanges
            .iter_mut()
            .find(|slot| {
                slot.exchange
                    .as_ref()
                    .is_some_and(|exchange| exchange.key() == key)
            })
            .ok_or(Error::UnknownQuestion)?;
        if !slot.authorized {
            return Err(Error::PendingAuthorization);
        }
        if matches!(&slot.exchange, Some(Exchange::Returned(response)) if response.await_method().is_some())
        {
            return Err(if slot.handoff.is_some() {
                Error::RedirectedResponse
            } else {
                Error::PendingResponse
            });
        }
        if !matches!(slot.exchange, Some(Exchange::Returned(_))) {
            return Err(Error::PendingResponse);
        }
        let Some(Exchange::Returned(response)) = slot.exchange.take() else {
            unreachable!()
        };
        Ok(response)
    }

    /// Maximum non-noop operations in a message's pipeline transform.
    pub fn path_capacity(&self) -> usize {
        self.message_path.maximum::<PATH>()
    }

    /// Maximum per-message capability receipts, including the incoming Call target.
    pub fn capability_capacity(&self) -> usize {
        self.message_capabilities.maximum::<CAPS>()
    }

    /// Number of allocated outgoing observer slots. Re-read after admissions
    /// when using growing storage. This is not the configured maximum.
    pub fn outgoing_capacity(&self) -> usize {
        self.outgoing.capacity()
    }
    /// Invalidate a partial idle scan after external work changes. Preserve the
    /// rotating cursor so repeated invalidation cannot starve later phases.
    pub fn invalidate_maintenance(&mut self) {
        self.scan_remaining = self.maintenance_total;
    }

    /// Poll one outgoing observer across the retained responses. Unrelated
    /// replies cannot settle it. Reflected routes retain their barrier inside
    /// the driver until ordered loopback output and its matching echo complete.
    pub fn poll_outgoing(
        &mut self,
        index: usize,
        messages: &messages::Messages<'pool, '_, '_>,
    ) -> Result<Propagation, Error> {
        self.check_live()?;
        if self.exchanges.iter().any(|slot| {
            matches!(&slot.exchange,
            Some(Exchange::Returned(response)) if self.outgoing.pending_for(response.key()))
        }) {
            self.reserve_barrier()?;
        }
        for slot in &*self.exchanges {
            if !slot.authorized {
                continue;
            }
            if let Some(Exchange::Returned(response)) = &slot.exchange {
                let progress =
                    if slot.canceled && slot.handoff.is_none() && response.await_method().is_some()
                    {
                        self.outgoing.poll_handoff::<S, _, CAPS>(
                            index,
                            response.key(),
                            Err(capnp::ErrorKind::Disconnected),
                            messages,
                            &mut self.runtime,
                            slot.retain_handoff_routes,
                        )
                    } else if let Some(result) = &slot.handoff {
                        self.outgoing.poll_handoff(
                            index,
                            response.key(),
                            result.as_ref().map_err(|e| *e),
                            messages,
                            &mut self.runtime,
                            slot.retain_handoff_routes,
                        )
                    } else {
                        self.outgoing.poll_one(
                            index,
                            response,
                            messages,
                            &mut self.runtime,
                            &mut self.imports,
                            &mut self.ledgers.imports,
                            &self.ledgers.exports,
                            &mut self.ledgers.answers,
                            &mut self.pipelines,
                        )
                    }
                    .map_err(Error::Outgoing)?;
                if !matches!(progress, outgoing::Progress::Unrelated) {
                    return Ok(match progress {
                        outgoing::Progress::Resolved(Some(barrier)) => {
                            self.hold_barrier(barrier);
                            Propagation::Resolved
                        }
                        outgoing::Progress::Resolved(None) => Propagation::Resolved,
                        outgoing::Progress::Pending => Propagation::Pending,
                        outgoing::Progress::Abandoned => Propagation::Abandoned,
                        outgoing::Progress::Unrelated => unreachable!(),
                    });
                }
            }
        }
        Ok(Propagation::Unrelated)
    }

    pub fn poll_pipeline(
        &mut self,
        index: usize,
        messages: &messages::Messages<'pool, '_, '_>,
    ) -> Result<Propagation, Error> {
        self.poll_pipeline_with(index, messages, |_, _, _| Ok(None))
    }
    /// Resolve early application pipelines through the existing answer observer
    /// registry. Resource errors remain retryable and accepted children retain
    /// their parent across Finish. The callback must preserve policy wrappers.
    #[allow(clippy::type_complexity)]
    pub fn poll_pipeline_with(
        &mut self,
        index: usize,
        messages: &messages::Messages<'pool, '_, '_>,
        resolve: impl FnOnce(
            &A,
            pipeline::Path<'_>,
            &mut Runtime<'pool, 'id, 's, I, P, S, CAPS, PATH>,
        ) -> Result<
            Option<Result<clients::Client<'id>, capnp::ErrorKind>>,
            pipelines::Error,
        >,
    ) -> Result<Propagation, Error> {
        self.check_live()?;
        let progress = if let Some(progress) = self.poll_redirect(index)? {
            progress
        } else {
            self.pipelines
                .poll_one_with(
                    index,
                    &mut self.runtime,
                    &mut self.ledgers.answers,
                    messages,
                    binding::Route::broken,
                    resolve,
                )
                .map_err(Error::Pipeline)?
        };
        Ok(match progress {
            pipelines::Progress::Vacant => Propagation::Unrelated,
            pipelines::Progress::Pending => Propagation::Pending,
            pipelines::Progress::Abandoned => Propagation::Abandoned,
            pipelines::Progress::Resolved(None) => Propagation::Resolved,
            // Local answer observers start without an original remote route.
            pipelines::Progress::Resolved(Some(_)) => {
                unreachable!("local pipeline has no prior route")
            }
        })
    }

    /// Let an integrated worker try direct local dispatch before queueing a
    /// Call. The caller must service `has_pending_call` before reading more
    /// Calls; ordinary maintenance still supplies the ordered fallback.
    pub fn defer_call_dispatch(&mut self, enabled: bool) {
        self.defer_calls = enabled;
    }

    /// Let the output FIFO own unobserved plain message leases directly. Its
    /// capacity remains the publication bound; these entries do not consume
    /// separately addressable message-pool send slots. Partial-write and terminal
    /// cleanup obligations are unchanged. Disabled for existing fixed adapters.
    pub fn use_owned_output_queues(&mut self, enabled: bool) {
        self.owned_output = enabled;
    }
    /// Whether plain publications are bounded by the output FIFO alone rather
    /// than also consuming separately addressable message-pool send slots.
    pub fn uses_owned_output_queues(&self) -> bool {
        self.owned_output
    }
    pub fn has_pending_call(&self) -> bool {
        self.call.is_some()
    }
    /// Whether the deferred request has no parameter capabilities, pipeline
    /// target or third-party answer. Such admission only retains its hosted
    /// target and creates its answer; it cannot resolve another capability.
    pub fn has_plain_pending_call(&self) -> bool {
        self.call.as_ref().is_some_and(|call| {
            call.capability_count() == 0
                && matches!(call.target(), TargetRef::Hosted(_))
                && self.ledgers.answers.is_third_party(call.answer()) == Ok(false)
        })
    }
    /// Accept a settled local target only when no older task is queued. The
    /// same graph ticket freezes its route, consumes sequence space and retains
    /// its target. Rejections leave the admitted Call available for retry.
    #[allow(clippy::type_complexity)]
    #[inline(always)]
    pub fn dispatch_immediate_call(
        &mut self,
        cancellation: bool,
    ) -> Result<
        Option<(
            dispatch::Running<'id, Request<'pool, 'id, I, CAPS, PATH>>,
            bool,
        )>,
        Error,
    > {
        self.check_live()?;
        let Some(call) = &self.call else {
            return Ok(None);
        };
        let TargetRef::Hosted(lease) = call.target() else {
            return Ok(None);
        };
        let key = call.answer();
        if self
            .ledgers
            .answers
            .is_third_party(key)
            .map_err(|e| Error::Input(super::Error::Admission(inbound::Error::Answer(e))))?
        {
            return Ok(None);
        }
        let Some(ticket) = self
            .runtime
            .prepare_immediate(self.ledgers.exports.get(lease))
            .map_err(Error::Dispatch)?
        else {
            return Ok(None);
        };
        if !matches!(
            ticket.destination(),
            clients::Destination::Endpoint {
                location: clients::Location::Local | clients::Location::Broken,
                ..
            }
        ) {
            return Ok(None);
        }
        let execute = self
            .ledgers
            .answers
            .dispatch(key, cancellation)
            .map_err(|e| Error::Input(super::Error::Admission(inbound::Error::Answer(e))))?;
        let call = self.call.take().expect("preflighted incoming Call");
        Ok(Some((ticket.accept_owned(call), execute)))
    }

    pub fn retry_call(&mut self) -> Result<Option<answers::Key<'id>>, Error> {
        self.check_live()?;
        if self.call.is_some() && !self.normal_dispatch_available() {
            return Err(Error::Enqueue(EnqueueError::Dispatch(
                dispatch::DispatchError::Full,
            )));
        }
        let Some(call) = self.call.take() else {
            return Ok(None);
        };
        let key = call.answer();
        match call.enqueue_wrapped(
            &mut self.runtime,
            &self.ledgers.exports,
            &mut self.ledgers.answers,
            &mut self.pipelines,
            Task::Call,
            |task| {
                task.into_call()
                    .unwrap_or_else(|_| unreachable!("unaccepted Call"))
            },
        ) {
            Ok(_) => Ok(Some(key)),
            Err(rejected) => {
                self.call = Some(rejected.call);
                Err(Error::Enqueue(rejected.error))
            }
        }
    }

    pub fn retry_resolution(
        &mut self,
        messages: &mut messages::Messages<'pool, '_, '_>,
    ) -> Result<bool, Error> {
        self.check_live()?;
        if self.resolution.is_some() {
            self.reserve_barrier()?;
        }
        let Some(resolution) = self.resolution.take() else {
            return Ok(false);
        };
        match resolution.apply(
            messages,
            &mut self.runtime,
            &mut self.imports,
            &mut self.ledgers.imports,
            &mut self.ledgers.exports,
            &mut self.ledgers.answers,
            &mut self.pipelines,
        ) {
            Ok(barrier) => {
                if let Some(barrier) = barrier {
                    self.hold_barrier(barrier);
                }
                Ok(true)
            }
            Err(rejected) => {
                self.resolution = Some(rejected.resolution);
                Err(Error::Resolve(rejected.error))
            }
        }
    }

    /// Admit each wire envelope once. Call, Bootstrap, Provide, Accept and Join invoke
    /// the producer factory; rejection returns that producer alongside the frame.
    /// Unsupported envelopes are preserved for the caller's protocol handling.
    #[allow(clippy::too_many_arguments, clippy::result_large_err)]
    pub fn receive(
        &mut self,
        frame: Frame<'pool>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        scratch: &mut [u32],
        mut make_import: impl FnMut(ImportId) -> I,
        make_producer: impl FnOnce() -> A,
    ) -> Result<Received<'id, A>, Rejected<'pool, A>> {
        self.receive_with_receiver(frame, messages, scratch, &mut make_import, make_producer)
    }
    #[allow(clippy::too_many_arguments, clippy::result_large_err)]
    pub fn receive_with_receiver(
        &mut self,
        frame: Frame<'pool>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        scratch: &mut [u32],
        receiver: &mut impl ImportReceiver<I>,
        make_producer: impl FnOnce() -> A,
    ) -> Result<Received<'id, A>, Rejected<'pool, A>> {
        self.receive_with_policy(frame, messages, scratch, receiver, |_| Ok(make_producer()))
    }

    /// Apply caller admission policy before accepting an incoming producer.
    /// The factory receives the complete serialized Call size in words (zero
    /// for control producers). Unreachable words are included. Rejection leaves
    /// the frame and protocol tables intact; Busy never invokes the factory.
    #[allow(clippy::too_many_arguments, clippy::result_large_err)]
    #[inline(always)]
    pub fn receive_with_policy(
        &mut self,
        frame: Frame<'pool>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        scratch: &mut [u32],
        receiver: &mut impl ImportReceiver<I>,
        make_producer: impl FnOnce(usize) -> capnp::Result<A>,
    ) -> Result<Received<'id, A>, Rejected<'pool, A>> {
        if self.disconnected.is_none() && !self.ledgers.questions.is_empty() {
            let blocked = self.resolution.is_some() || self.exchanges.iter().any(|slot| {
                matches!(&slot.exchange, Some(Exchange::Returned(response))
                    if (response.requires_ordered_propagation() || slot.handoff.is_some())
                        && (self.outgoing.pending_for(response.key()) || self.invocation_pipelines.iter().any(|slot| slot.pending_for(response.key()))))
            });
            if !blocked {
                let admitted = frame
                    .with(messages, |root| -> Result<_, super::Error> {
                        use capnp::traits::IntoInternalStructReader;
                        let fields =
                            rpc_capnp::api::MessageRef::from(root.into_internal_struct_reader());
                        if fields.tag() != rpc_capnp::api::MessageUnionTag::Return {
                            return Ok(None);
                        }
                        let result = rpc_capnp::return_::Reader::from(
                            fields.r#return()?.into_internal_struct_reader(),
                        );
                        let id = result.get_answer_id();
                        let Some((index, question)) =
                            self.exchanges
                                .iter()
                                .enumerate()
                                .find_map(|(index, slot)| match &slot.exchange {
                                    Some(Exchange::Pending(question))
                                        if question.key().id().to_wire() == id =>
                                    {
                                        Some((index, question))
                                    }
                                    _ => None,
                                })
                        else {
                            return Ok(None);
                        };
                        question
                            .try_admit_plain_return(
                                result,
                                &frame,
                                messages,
                                &mut self.ledgers.questions,
                                false,
                            )
                            .map(|admitted| admitted.map(|admitted| (index, admitted)))
                    })
                    .and_then(core::convert::identity);
                match admitted {
                    Ok(Some((index, admitted))) => {
                        // The exclusive receive operation has not exposed a callback
                        // between locating the owner and committing its admission.
                        let slot = &mut self.exchanges[index];
                        let Some(Exchange::Pending(question)) = slot.exchange.take() else {
                            unreachable!("admitted plain reply owner")
                        };
                        let response =
                            question.finish_return(frame, messages, admitted.into_general());
                        let key = response.key();
                        slot.exchange = Some(Exchange::Returned(response));
                        return Ok(Received::Returned(key));
                    }
                    Ok(None) => (),
                    Err(error) => {
                        return Err(Rejected {
                            error: Error::Input(error),
                            frame,
                            producer: None,
                        })
                    }
                }
            }
        }
        let mut make_producer = Some(make_producer);
        if self.defer_calls
            && self.disconnected.is_none()
            && self.call.is_none()
            && self.echo.is_none()
        {
            // The hosted worker owns dispatch. Ordinary Call admission can
            // commit directly to its pending slot without constructing the
            // general control/Return classification and its optional owners.
            // The same decoder and two-ledger transaction enforce every limit.
            let mut owner = None;
            let admitted = frame
                .with_size(messages, |root, words| {
                    use capnp::traits::IntoInternalStructReader;
                    let fields =
                        rpc_capnp::api::MessageRef::from(root.into_internal_struct_reader());
                    if fields.tag() != rpc_capnp::api::MessageUnionTag::Call {
                        return Ok(None);
                    }
                    let producer = make_producer.take().unwrap()(words)?;
                    owner = Some(answer::State::new(pipelines::Results::new(producer)));
                    let rpc_capnp::message::Call(call) =
                        root.which().map_err(|_| super::Error::WrongMessage)?
                    else {
                        unreachable!("classified Call")
                    };
                    Request::admit_decoded(
                        &frame,
                        messages,
                        call?,
                        &mut self.ledgers.imports,
                        &mut self.ledgers.exports,
                        &mut self.ledgers.answers,
                        &mut owner,
                        receiver,
                        self.message_capabilities,
                        self.message_path,
                    )
                    .map(Some)
                })
                .and_then(core::convert::identity);
            match admitted {
                Ok(Some(admitted)) => {
                    let call = admitted.finish(frame);
                    let key = call.answer();
                    self.call = Some(call);
                    return Ok(Received::DeferredCall(key));
                }
                Err(error) => {
                    return Err(Rejected {
                        error: Error::Input(error),
                        frame,
                        producer: owner.map(|owner| {
                            owner.release(messages).release(messages, &mut self.runtime)
                        }),
                    })
                }
                Ok(None) => (),
            }
        }
        self.receive_general(frame, messages, scratch, receiver, make_producer.unwrap())
    }

    #[allow(clippy::too_many_arguments, clippy::result_large_err)]
    #[inline(never)]
    fn receive_general(
        &mut self,
        frame: Frame<'pool>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        scratch: &mut [u32],
        receiver: &mut impl ImportReceiver<I>,
        make_producer: impl FnOnce(usize) -> capnp::Result<A>,
    ) -> Result<Received<'id, A>, Rejected<'pool, A>> {
        if let Err(error) = self.check_live() {
            return Err(Rejected {
                error,
                frame,
                producer: None,
            });
        }
        use IncomingKind as Kind;
        let propagation_blocked = self.resolution.is_some() || self.exchanges.iter().any(|slot| {
            matches!(&slot.exchange, Some(Exchange::Returned(response))
                if (response.requires_ordered_propagation() || slot.handoff.is_some()) && (self.outgoing.pending_for(response.key()) || self.invocation_pipelines.iter().any(|slot| slot.pending_for(response.key()))))
        });
        let mut admitted_return = None;
        let mut admitted_call = None;
        let mut call_owner = None;
        let mut make_producer = Some(make_producer);
        let kind = frame
            .with_size(messages, |root, words| -> Result<_, super::Error> {
                use capnp::traits::IntoInternalStructReader;
                use rpc_capnp::api::{MessageRef, MessageUnionTag, ReturnRef, ReturnUnionTag};
                // Classify without traversing the payload of request/control
                // messages which their admission transaction will read below.
                // A tag never certifies the corresponding pointer or body.
                let fields = MessageRef::from(root.into_internal_struct_reader());
                match fields.tag() {
                    MessageUnionTag::Return => {
                        let result = rpc_capnp::return_::Reader::from(
                            fields.r#return()?.into_internal_struct_reader(),
                        );
                        let id = result.get_answer_id();
                        let awaiting = ReturnRef::from(result.into_internal_struct_reader()).tag()
                            == ReturnUnionTag::AwaitFromThirdParty;
                        if !awaiting && !propagation_blocked {
                            if let Some(question) =
                                self.exchanges.iter().find_map(|slot| match &slot.exchange {
                                    Some(Exchange::Pending(question))
                                        if question.key().id().to_wire() == id =>
                                    {
                                        Some(question)
                                    }
                                    _ => None,
                                })
                            {
                                admitted_return = Some(question.admit_return(
                                    result,
                                    &frame,
                                    messages,
                                    &mut self.ledgers.questions,
                                    &mut self.ledgers.imports,
                                    &mut self.ledgers.exports,
                                    &mut self.ledgers.answers,
                                    scratch,
                                    receiver,
                                    self.message_capabilities,
                                )?);
                            }
                        }
                        return Ok(Kind::Return(id, awaiting));
                    }

                    MessageUnionTag::Call => {
                        if self.call.is_none() && self.echo.is_none() {
                            let producer = make_producer.take().unwrap()(words)?;
                            call_owner =
                                Some(answer::State::new(pipelines::Results::new(producer)));
                            let rpc_capnp::message::Call(call) =
                                root.which().map_err(|_| super::Error::WrongMessage)?
                            else {
                                unreachable!("classified Call");
                            };
                            admitted_call = Some(Request::admit_decoded(
                                &frame,
                                messages,
                                call?,
                                &mut self.ledgers.imports,
                                &mut self.ledgers.exports,
                                &mut self.ledgers.answers,
                                &mut call_owner,
                                receiver,
                                self.message_capabilities,
                                self.message_path,
                            )?);
                        }
                        return Ok(Kind::Call(words));
                    }
                    MessageUnionTag::Bootstrap => return Ok(Kind::Bootstrap),
                    MessageUnionTag::Provide => return Ok(Kind::Provide),
                    MessageUnionTag::Accept => return Ok(Kind::Accept),
                    MessageUnionTag::ThirdPartyAnswer => return Ok(Kind::ThirdPartyAnswer),
                    MessageUnionTag::Join => return Ok(Kind::Join),
                    MessageUnionTag::Resolve => return Ok(Kind::Resolve),
                    MessageUnionTag::Release => return Ok(Kind::Release),
                    _ => (),
                }
                Ok(
                    match root.which().map_err(|_| super::Error::WrongMessage)? {
                        rpc_capnp::message::Call(_) => Kind::Call(words),
                        rpc_capnp::message::Bootstrap(_) => Kind::Bootstrap,
                        rpc_capnp::message::Provide(_) => Kind::Provide,
                        rpc_capnp::message::Accept(_) => Kind::Accept,
                        rpc_capnp::message::ThirdPartyAnswer(_) => Kind::ThirdPartyAnswer,
                        rpc_capnp::message::Join(_) => Kind::Join,
                        rpc_capnp::message::Resolve(_) => Kind::Resolve,
                        rpc_capnp::message::Finish(finish) => {
                            let finish = finish?;
                            Kind::Finish(
                                AnswerId::from_wire(finish.get_question_id()),
                                finish.get_release_result_caps(),
                            )
                        }
                        rpc_capnp::message::Release(_) => Kind::Release,
                        rpc_capnp::message::Abort(exception) => {
                            Kind::Abort(controls::abort_reason(exception?)?)
                        }
                        rpc_capnp::message::Unimplemented(message) => {
                            match controls::unimplemented(message?)? {
                                controls::Unimplemented::Resolve(export) => {
                                    Kind::Unimplemented(export)
                                }
                                controls::Unimplemented::Required => {
                                    Kind::Abort(capnp::ErrorKind::Unimplemented)
                                }
                            }
                        }
                        rpc_capnp::message::Disembargo(disembargo) => {
                            match disembargo?
                                .get_context()
                                .which()
                                .map_err(|_| super::Error::WrongMessage)?
                            {
                                rpc_capnp::disembargo::context::SenderLoopback(_) => {
                                    Kind::SenderLoopback
                                }
                                rpc_capnp::disembargo::context::ReceiverLoopback(id) => {
                                    Kind::ReceiverLoopback(id)
                                }
                                rpc_capnp::disembargo::context::Accept(_) => Kind::ProvisionBarrier,
                            }
                        }
                        _ => return Err(super::Error::WrongMessage),
                    },
                )
            })
            .and_then(core::convert::identity);
        let kind = match kind {
            Ok(kind) => kind,
            Err(error) => {
                return Err(Rejected {
                    error: Error::Input(error),
                    frame,
                    producer: call_owner
                        .map(|owner| owner.release(messages).release(messages, &mut self.runtime)),
                })
            }
        };
        // Propagate a Return's routes before admitting a following Return or
        // Resolve. Otherwise Resolve can be consumed while its newly returned
        // import still has no graph observer, losing the resolution's meaning.
        // Cleanup remains admissible when propagation needs resource capacity;
        // local tail adoption must not block replies needed by its producer.
        if matches!(kind, Kind::Return(..) | Kind::Resolve) && propagation_blocked {
            return Err(Rejected {
                error: Error::Busy,
                frame,
                producer: None,
            });
        }
        if let Some(admitted) = admitted_call {
            let call = admitted.finish(frame);
            let key = call.answer();
            self.call = Some(call);
            if self.defer_calls {
                return Ok(Received::DeferredCall(key));
            }
            return match self.retry_call() {
                Ok(_) => Ok(Received::Queued(key)),
                Err(_) => Ok(Received::DeferredCall(key)),
            };
        }
        match kind {
            Kind::Return(id, awaiting) => {
                if awaiting && self.network.is_some() {
                    return Err(Rejected {
                        error: Error::Busy,
                        frame,
                        producer: None,
                    });
                }
                let metadata = if awaiting {
                    match messages.retain(&frame.lease) {
                        Ok(lease) => Some(Frame::new(lease, frame.options)),
                        Err(error) => {
                            return Err(Rejected {
                                error: Error::Input(super::Error::Storage(error)),
                                frame,
                                producer: None,
                            })
                        }
                    }
                } else {
                    None
                };
                let Some(slot) = self.exchanges.iter_mut().find(|slot| matches!(&slot.exchange, Some(Exchange::Pending(question)) if question.key().id().to_wire() == id)) else {
                    if let Some(metadata) = metadata { metadata.release(messages); }
                    return Err(Rejected { error: Error::UnknownQuestion, frame, producer: None })
                };
                let Some(Exchange::Pending(question)) = slot.exchange.take() else {
                    unreachable!()
                };
                let returned = match admitted_return {
                    Some(admitted) => Ok(question.finish_return(frame, messages, admitted)),
                    None => question.returned_with_receiver(
                        frame,
                        messages,
                        &mut self.ledgers.questions,
                        &mut self.ledgers.imports,
                        &mut self.ledgers.exports,
                        &mut self.ledgers.answers,
                        scratch,
                        receiver,
                        self.message_capabilities,
                    ),
                };
                match returned {
                    Ok(response) => {
                        let key = response.key();
                        let awaiting = response.await_method().is_some();
                        if let Some(metadata) = metadata {
                            if awaiting {
                                self.network = Some(NetworkRequest::Await(AwaitingResult {
                                    frame: metadata,
                                    key,
                                }));
                            } else {
                                metadata.release(messages);
                            }
                        }
                        slot.exchange = Some(Exchange::Returned(response));
                        if awaiting {
                            Ok(Received::Network)
                        } else {
                            Ok(Received::Returned(key))
                        }
                    }
                    Err(rejected) => {
                        if let Some(metadata) = metadata {
                            metadata.release(messages);
                        }
                        slot.exchange = Some(Exchange::Pending(rejected.question));
                        Err(Rejected {
                            error: Error::Input(rejected.error),
                            frame: rejected.frame,
                            producer: None,
                        })
                    }
                }
            }
            other => self.receive_control(
                other,
                frame,
                messages,
                scratch,
                receiver,
                make_producer.expect("unconsumed control producer"),
            ),
        }
    }

    // The common reply path above shares its checked root with admission.
    // Keep unrelated request/control transactions in their own stack frame.
    #[allow(clippy::too_many_arguments, clippy::result_large_err)]
    #[inline(never)]
    fn receive_control(
        &mut self,
        kind: IncomingKind,
        frame: Frame<'pool>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        scratch: &mut [u32],
        receiver: &mut impl ImportReceiver<I>,
        make_producer: impl FnOnce(usize) -> capnp::Result<A>,
    ) -> Result<Received<'id, A>, Rejected<'pool, A>> {
        use IncomingKind as Kind;
        match kind {
            Kind::ProvisionBarrier => {
                self.receive_provision_barrier(frame, messages)?;
                Ok(Received::QueuedNetwork)
            }
            Kind::ThirdPartyAnswer => {
                if self.network.is_some() {
                    return Err(Rejected {
                        error: Error::Busy,
                        frame,
                        producer: None,
                    });
                }
                let Some(slot) = self
                    .exchanges
                    .vacant_mut(|_, slot| slot.exchange.is_none(), || ExchangeSlot::EMPTY)
                else {
                    return Err(Rejected {
                        error: Error::Busy,
                        frame,
                        producer: None,
                    });
                };
                match Question::adopt(frame, messages, &mut self.ledgers.questions) {
                    Ok((question, job)) => {
                        slot.exchange = Some(Exchange::Pending(question));
                        slot.authorized = false;
                        slot.canceled = false;
                        slot.tail_taken = false;
                        self.network = Some(NetworkRequest::Adopt(job));
                        Ok(Received::Network)
                    }
                    Err(rejected) => Err(Rejected {
                        error: Error::Input(rejected.error),
                        frame: rejected.frame,
                        producer: None,
                    }),
                }
            }
            Kind::Provide | Kind::Accept | Kind::Join => {
                if self.network.is_some() {
                    return Err(Rejected {
                        error: Error::Busy,
                        frame,
                        producer: None,
                    });
                }
                let producer = match make_producer(0) {
                    Ok(producer) => producer,
                    Err(error) => {
                        return Err(Rejected {
                            error: Error::Input(error.into()),
                            frame,
                            producer: None,
                        })
                    }
                };
                let owner = answer::State::new(pipelines::Results::new(producer));
                let request = match kind {
                    Kind::Provide => super::provision::Provide::admit_with_capacity(
                        frame,
                        messages,
                        &mut self.ledgers.exports,
                        &mut self.ledgers.answers,
                        owner,
                        self.message_path,
                    )
                    .map(NetworkRequest::Provide),
                    Kind::Accept => super::provision::Accept::admit(
                        frame,
                        messages,
                        &mut self.ledgers.answers,
                        owner,
                    )
                    .map(NetworkRequest::Accept),
                    Kind::Join => super::join::Join::admit_with_capacity(
                        frame,
                        messages,
                        &mut self.ledgers.exports,
                        &mut self.ledgers.answers,
                        owner,
                        self.message_path,
                    )
                    .map(NetworkRequest::Join),
                    _ => unreachable!(),
                };
                match request {
                    Ok(request) => {
                        self.network = Some(request);
                        Ok(Received::Network)
                    }
                    Err(rejected) => Err(Rejected {
                        error: Error::Input(rejected.error),
                        frame: rejected.frame,
                        producer: Some(
                            rejected
                                .owner
                                .release(messages)
                                .release(messages, &mut self.runtime),
                        ),
                    }),
                }
            }
            Kind::Abort(reason) => {
                self.disconnect(reason);
                frame.release(messages);
                Ok(Received::Aborted(reason))
            }
            Kind::Unimplemented(export) => {
                if let Some(id) = export {
                    match self.ledgers.exports.release(id, 1) {
                        Ok(Some(client)) => self.runtime.release(client),
                        Ok(None) => (),
                        Err(error) => {
                            return Err(Rejected {
                                error: Error::Input(super::Error::Admission(
                                    inbound::Error::Export(error),
                                )),
                                frame,
                                producer: None,
                            })
                        }
                    }
                }
                frame.release(messages);
                Ok(Received::Unimplemented)
            }
            Kind::Call(words) => {
                if self.call.is_some() || self.echo.is_some() {
                    return Err(Rejected {
                        error: Error::Busy,
                        frame,
                        producer: None,
                    });
                }
                let producer = match make_producer(words) {
                    Ok(producer) => producer,
                    Err(error) => {
                        return Err(Rejected {
                            error: Error::Input(error.into()),
                            frame,
                            producer: None,
                        })
                    }
                };
                let call = match Request::admit_with_receiver(
                    frame,
                    messages,
                    &mut self.ledgers.imports,
                    &mut self.ledgers.exports,
                    &mut self.ledgers.answers,
                    answer::State::new(pipelines::Results::new(producer)),
                    receiver,
                    self.message_capabilities,
                    self.message_path,
                ) {
                    Ok(call) => call,
                    Err(rejected) => {
                        return Err(Rejected {
                            error: Error::Input(rejected.error),
                            frame: rejected.frame,
                            producer: Some(
                                rejected
                                    .owner
                                    .release(messages)
                                    .release(messages, &mut self.runtime),
                            ),
                        })
                    }
                };
                let key = call.answer();
                self.call = Some(call);
                if self.defer_calls {
                    return Ok(Received::DeferredCall(key));
                }
                match self.retry_call() {
                    Ok(_) => Ok(Received::Queued(key)),
                    Err(_) => Ok(Received::DeferredCall(key)),
                }
            }
            Kind::Bootstrap => {
                let producer = match make_producer(0) {
                    Ok(producer) => producer,
                    Err(error) => {
                        return Err(Rejected {
                            error: Error::Input(error.into()),
                            frame,
                            producer: None,
                        })
                    }
                };
                match controls::bootstrap(
                    frame,
                    messages,
                    &mut self.ledgers.answers,
                    answer::State::new(pipelines::Results::new(producer)),
                ) {
                    Ok(key) => Ok(Received::Bootstrap(Bootstrap { key })),
                    Err(rejected) => Err(Rejected {
                        error: Error::Input(rejected.error),
                        frame: rejected.frame,
                        producer: Some(
                            rejected
                                .owner
                                .release(messages)
                                .release(messages, &mut self.runtime),
                        ),
                    }),
                }
            }
            Kind::Return(..) => unreachable!("reply dispatched before controls"),
            Kind::Resolve => {
                if self.resolution.is_some() {
                    return Err(Rejected {
                        error: Error::Busy,
                        frame,
                        producer: None,
                    });
                }
                let resolution = Resolution::admit_with_receiver(
                    frame,
                    messages,
                    &mut self.ledgers.imports,
                    &mut self.ledgers.exports,
                    &mut self.ledgers.answers,
                    receiver,
                )
                .map_err(|rejected| Rejected {
                    error: Error::Input(rejected.error),
                    frame: rejected.frame,
                    producer: None,
                })?;
                self.resolution = Some(resolution);
                match self.retry_resolution(messages) {
                    Ok(_) => Ok(Received::Resolved),
                    Err(_) => Ok(Received::DeferredResolution),
                }
            }
            Kind::Finish(id, release_result_caps) => {
                let outcome = answer::finished_parts(
                    id,
                    release_result_caps,
                    messages,
                    &mut self.ledgers.answers,
                    &mut self.ledgers.exports,
                    scratch,
                );
                match outcome {
                    Ok(finished) => {
                        frame.release(messages);
                        Ok(Received::Finished(
                            finished.and_then(|value| value.retired).map(|state| {
                                state.release(messages).release(messages, &mut self.runtime)
                            }),
                        ))
                    }
                    Err(error) => Err(Rejected {
                        error: Error::Input(error),
                        frame,
                        producer: None,
                    }),
                }
            }
            Kind::Release => {
                let outcome = frame
                    .with(messages, |root| -> Result<_, super::Error> {
                        let rpc_capnp::message::Release(release) =
                            root.which().map_err(|_| super::Error::WrongMessage)?
                        else {
                            unreachable!()
                        };
                        inbound::released(release?, &mut self.ledgers.exports, scratch)
                            .map_err(super::Error::Admission)?
                            .accept();
                        Ok(())
                    })
                    .and_then(core::convert::identity);
                match outcome {
                    Ok(()) => {
                        frame.release(messages);
                        Ok(Received::Released)
                    }
                    Err(error) => Err(Rejected {
                        error: Error::Input(error),
                        frame,
                        producer: None,
                    }),
                }
            }
            Kind::SenderLoopback => {
                // An echo cannot overtake a prior request still waiting for a
                // dispatch slot. Later Calls similarly wait for this echo.
                if self.call.is_some() || self.echo.is_some() {
                    return Err(Rejected {
                        error: Error::Busy,
                        frame,
                        producer: None,
                    });
                }
                let echo = super::loopback::Echo::admit_with_capacity(
                    frame,
                    messages,
                    &mut self.ledgers.exports,
                    &mut self.ledgers.answers,
                    self.message_path,
                )
                .map_err(|rejected| Rejected {
                    error: Error::Input(rejected.error),
                    frame: rejected.frame,
                    producer: None,
                })?;
                self.echo = Some(echo);
                // Successful admission retains the target exactly once even
                // when its graph binding or dispatch queue needs a later retry.
                let _ = self.retry_echo();
                Ok(Received::Loopback)
            }
            Kind::ReceiverLoopback(id) => {
                match self
                    .loopbacks
                    .receive(capntproto_rpc_engine::ids::EmbargoId::from_wire(id))
                {
                    Ok(barrier) => {
                        self.runtime.open(barrier);
                        frame.release(messages);
                        Ok(Received::Loopback)
                    }
                    Err(error) => Err(Rejected {
                        error: Error::Loopback(error),
                        frame,
                        producer: None,
                    }),
                }
            }
        }
    }

    pub fn release_response(
        &mut self,
        response: Response<'pool, 'id, I, CAPS>,
        messages: &mut messages::Messages<'pool, '_, '_>,
    ) {
        response.release(
            messages,
            &mut self.ledgers.questions,
            &mut self.ledgers.imports,
            &mut self.ledgers.exports,
            &mut self.ledgers.answers,
        );
    }
    #[inline(always)]
    pub fn release_call(
        &mut self,
        call: Request<'pool, 'id, I, CAPS, PATH>,
        messages: &mut messages::Messages<'pool, '_, '_>,
    ) {
        call.release(
            messages,
            &mut self.ledgers.imports,
            &mut self.ledgers.exports,
            &mut self.ledgers.answers,
        );
    }
    pub fn drain_canceled_response(
        &mut self,
        messages: &mut messages::Messages<'pool, '_, '_>,
    ) -> bool {
        let mut canceled = false;
        let slot = self.exchanges.iter_mut().find(|slot| {
            let live = slot.canceled && slot.exchange.is_some();
            canceled |= live;
            live && matches!(&slot.exchange, Some(Exchange::Returned(response)) if !self.outgoing.pending_for(response.key()) && !self.invocation_pipelines.iter().any(|slot| slot.pending_for(response.key())))
        });
        let Some(slot) = slot else {
            self.canceled_exchanges = canceled;
            return false;
        };
        slot.canceled = false;
        let Some(Exchange::Returned(response)) = slot.exchange.take() else {
            unreachable!()
        };
        let handoff = slot.handoff.take();
        if let Some(Ok(result)) = handoff {
            result.release(messages, &mut self.runtime);
        }
        self.release_response(response, messages);
        true
    }
    pub fn drain_export(&mut self, index: usize) -> bool {
        let Some(client) = self.ledgers.exports.take_retired(index) else {
            return false;
        };
        self.runtime.release(client);
        true
    }
    pub fn drain_endpoint(&mut self) -> Option<Option<S>> {
        self.runtime.take_retired().map(|endpoint| {
            endpoint.release(&mut self.ledgers.imports, &mut self.ledgers.questions)
        })
    }
    pub fn drain_answer(
        &mut self,
        index: usize,
        messages: &mut messages::Messages<'pool, '_, '_>,
    ) -> Option<A> {
        self.ledgers
            .answers
            .take_finished(index)
            .map(|state| state.release(messages).release(messages, &mut self.runtime))
    }
    /// Record the first terminal transport/protocol error. No coordinator
    /// operation may publish or admit further work; existing owners remain
    /// available to consuming terminal cleanup. The caller stops transport IO.
    pub fn disconnect(&mut self, reason: capnp::ErrorKind) {
        self.disconnected.get_or_insert(reason);
    }
    pub fn disconnected(&self) -> Option<capnp::ErrorKind> {
        self.disconnected
    }
    fn check_live(&self) -> Result<(), Error> {
        self.disconnected
            .map_or(Ok(()), |reason| Err(Error::Disconnected(reason)))
    }
}

mod join;
mod loopbacks;
mod network;
pub use join::JoinRejected;
mod provision_barrier;
mod vine_barrier;
pub use network::{
    AcceptFailureRejected, AcceptRejected, AdoptedAnswerRejected, AdoptionError, AdoptionRejected,
    NetworkRequest, RegistrationError, RegistrationRejected,
};
pub use provision_barrier::{
    EmbargoError, EmbargoRejected, ProvisionBarrier, QueuedProvisionBarrier,
};
pub use vine_barrier::{QueuedVineBarrier, VineBarrier, VineRejected};
mod maintenance;
mod terminated;
pub use terminated::{Terminating, Termination};
mod tail;
pub use tail::TailSlot;
mod invocation;
pub use invocation::{EncodedCall, ImmediateParams, Invocation, InvocationParams, SentInvocation};
#[cfg(feature = "alloc")]
pub use invocation::{PreparedCall, PreparedCallTemplate};
mod invocation_pipeline;
pub use invocation_pipeline::InvocationPipelineSlot;
mod replies;
pub use replies::{
    RejectedOutputReply, RejectedReply, Replies, ReplyError, ReplyProgress, ReplySlot,
};
mod local_result;
pub use local_result::{LocalPayload, LocalResult};
#[cfg(feature = "alloc")]
mod prepared_return;
#[cfg(feature = "alloc")]
pub use prepared_return::{PreparedReturn, PreparedReturnTemplate};
mod bootstrap;
mod result_draft;
mod server_output;
pub use bootstrap::Bootstrap;
pub use result_draft::{
    DraftError, LocalResultDraft, RejectedResultCapability, ResultCapabilities,
};
mod awaiting;
mod call_output;
pub use awaiting::{AnswerHandoffRejected, AwaitingResult, HandoffError, HandoffRejected};
pub use call_output::{CallOutput, CallResult};
mod session;
pub use session::{
    AccessRejected, CallError, Drive, PendingAck, PendingReply, ReleaseRejected, ResultReader,
    Session, Streaming, StreamingAck, StreamingSlot, TypedReply,
};
mod invocations;
pub use invocations::{
    CallProgress, InvocationSlot, Invocations, LocalCall, RejectedInvocation, RejectedLocal,
};

mod fixed;
pub use fixed::{FixedStorage, InitializeError};

#[cfg(feature = "alloc")]
mod growing;
#[cfg(feature = "alloc")]
pub use growing::GrowingStorage;
