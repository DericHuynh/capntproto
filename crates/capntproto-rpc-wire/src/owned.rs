//! Owning input tasks: immutable frames and their admitted capability receipts
//! move together, without self-references, allocation or an executor.
#![forbid(unsafe_code)]
use crate::{attachments::ImportReceiver, calls, inbound, pipeline, reception, rpc_capnp};
use capntproto_rpc_engine::{
    answers, exports,
    ids::{AnswerId, ExportId, ImportId, WireId},
    imports, messages,
};
pub mod join;
pub mod provision;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Wire(capnp::ErrorKind),
    Storage(messages::Error),
    Admission(inbound::Error),
    Output(crate::admission::Error),
    Target(pipeline::Error),
    Capacity,
    TrailingBytes,
    WrongMessage,
}
impl From<capnp::Error> for Error {
    fn from(error: capnp::Error) -> Self {
        Self::Wire(error.kind)
    }
}
impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "owned RPC input: {self:?}")
    }
}
impl core::error::Error for Error {}

/// The lease is deliberately private. A suspended/queued request cannot have
/// its bytes replaced independently of its admitted receipts.
#[must_use = "admit or release this frame through its originating message pool"]
pub struct Frame<'pool> {
    lease: messages::Lease<'pool>,
    options: capnp::message::ReaderOptions,
    // A successful framing pass certifies this immutable lease and its fixed
    // reader options. Cache only the common single-segment layout; pointer
    // traversal still gets a fresh arena and the original limits on every read.
    single_bytes: core::cell::Cell<Option<core::num::NonZeroUsize>>,
}
impl<'pool> Frame<'pool> {
    /// Copy a complete stream frame into caller storage, checking the segment
    /// table, exact length and reader limits before returning an immutable owner.
    /// Pointer and capability validation still occurs during protocol admission.
    #[inline(always)]
    pub fn copy_from(
        messages: &mut messages::Messages<'pool, '_, '_>,
        bytes: &[u8],
        options: capnp::message::ReaderOptions,
    ) -> Result<Self, Error> {
        let lease = messages.copy_from(bytes).map_err(Error::Storage)?;
        let frame = Self::new(lease, options);
        if let Err(error) = frame.size_in_words(messages) {
            frame.release(messages);
            return Err(error);
        }
        Ok(frame)
    }

    pub fn new(lease: messages::Lease<'pool>, options: capnp::message::ReaderOptions) -> Self {
        Self {
            lease,
            options,
            single_bytes: core::cell::Cell::new(None),
        }
    }
    #[inline(always)]
    fn reader<'a>(
        &self,
        view: &'a [u8],
    ) -> Result<capnp::message::Reader<capnp::serialize::NoAllocSliceSegments<'a>>, Error> {
        use capnp::message::ReaderSegments;
        if let Some(length) = self.single_bytes.get() {
            return Ok(capnp::message::Reader::new(
                capnp::serialize::NoAllocBufferSegments::from_segment_table_info(
                    view,
                    capnp::serialize::NoAllocSegmentTableInfo {
                        segments_count: 1,
                        segment_table_length_bytes: 8,
                        total_segments_length_bytes: length.get() - 8,
                    },
                ),
                self.options,
            ));
        }
        let mut bytes = view;
        let reader =
            capnp::serialize::read_message_from_flat_slice_no_alloc(&mut bytes, self.options)?;
        if !bytes.is_empty() {
            return Err(Error::TrailingBytes);
        }
        if reader.get_segments().len() == 1 {
            self.single_bytes
                .set(core::num::NonZeroUsize::new(view.len()));
        }
        Ok(reader)
    }
    /// Build the single segment directly in its reserved wire buffer. The
    /// closure cannot publish a partial frame; every failure recovers storage.
    pub fn build(
        messages: &mut messages::Messages<'pool, '_, '_>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        fill: impl FnOnce(&mut capnp::message::FixedBuilder<'_>) -> capnp::Result<()>,
    ) -> Result<Self, Error> {
        let writer = messages.reserve(minimum.max(16)).map_err(Error::Storage)?;
        Self::build_reserved(messages, writer, options, fill)
    }
    /// Construct a frame with an exact capacity bound, clearing only the bytes
    /// exposed to its builder. Useful for fixed-size controls and sized output.
    pub fn build_bounded(
        messages: &mut messages::Messages<'pool, '_, '_>,
        capacity: usize,
        options: capnp::message::ReaderOptions,
        fill: impl FnOnce(&mut capnp::message::FixedBuilder<'_>) -> capnp::Result<()>,
    ) -> Result<Self, Error> {
        let writer = messages
            .reserve_exact(capacity.max(16))
            .map_err(Error::Storage)?;
        Self::build_reserved(messages, writer, options, fill)
    }
    fn build_reserved(
        messages: &mut messages::Messages<'pool, '_, '_>,
        writer: messages::Writer<'pool>,
        options: capnp::message::ReaderOptions,
        fill: impl FnOnce(&mut capnp::message::FixedBuilder<'_>) -> capnp::Result<()>,
    ) -> Result<Self, Error> {
        let result = (|| {
            let mut view = messages.try_write(&writer).map_err(Error::Storage)?;
            let length = {
                let mut builder = capnp::message::FixedBuilder::from_bytes(&mut view[8..])?;
                fill(&mut builder)?;
                builder.into_segment().len()
            };
            let words = u32::try_from(length / 8)
                .map_err(|_| Error::Wire(capnp::ErrorKind::MessageSizeOverflow))?;
            view[..4].copy_from_slice(&0u32.to_le_bytes());
            view[4..8].copy_from_slice(&words.to_le_bytes());
            Ok(length + 8)
        })();
        let length = match result {
            Ok(length) => length,
            Err(error) => {
                messages.discard(writer);
                return Err(error);
            }
        };
        match messages.seal(writer, length) {
            Ok(lease) => Ok(Self {
                lease,
                options,
                single_bytes: core::cell::Cell::new(
                    if options
                        .traversal_limit_in_words
                        .is_none_or(|limit| (length - 8) / 8 <= limit)
                    {
                        core::num::NonZeroUsize::new(length)
                    } else {
                        None
                    },
                ),
            }),
            Err(rejected) => {
                messages.discard(rejected.writer);
                Err(Error::Storage(rejected.error))
            }
        }
    }
    pub fn release(self, messages: &mut messages::Messages<'pool, '_, '_>) {
        messages.release(self.lease);
    }
    fn offer(
        self,
        messages: &mut messages::Messages<'pool, '_, '_>,
    ) -> Result<messages::Offer<'pool>, Error> {
        messages.offer_owned(self.lease).map_err(|rejected| {
            messages.release(rejected.lease);
            Error::Storage(rejected.error)
        })
    }
    /// Retain immutable allocated bytes independently of this frame's lease.
    /// Caller-owned fixed storage cannot escape its scope and returns `None`.
    #[cfg(feature = "alloc")]
    pub fn share(
        &self,
        messages: &messages::Messages<'pool, '_, '_>,
    ) -> Option<messages::SharedBytes> {
        messages.share(&self.lease)
    }
    #[cfg(feature = "alloc")]
    #[inline(always)]
    fn share_segments(
        &self,
        messages: &messages::Messages<'pool, '_, '_>,
    ) -> Result<Option<capnp::serialize::NoAllocBufferSegments<messages::SharedBytes>>, Error> {
        let Some(bytes) = self.share(messages) else {
            return Ok(None);
        };
        let segments = if let Some(length) = self.single_bytes.get() {
            // The private immutable lease and its options are the same ones
            // whose framing was checked by reader(). Sharing cannot edit them.
            capnp::serialize::NoAllocBufferSegments::from_segment_table_info(
                bytes,
                capnp::serialize::NoAllocSegmentTableInfo {
                    segments_count: 1,
                    segment_table_length_bytes: 8,
                    total_segments_length_bytes: length.get() - 8,
                },
            )
        } else {
            capnp::serialize::NoAllocBufferSegments::from_buffer_exact(bytes, self.options)?
        };
        Ok(Some(segments))
    }
    #[cfg(feature = "alloc")]
    fn retain_plain_payload(
        &self,
        messages: &messages::Messages<'pool, '_, '_>,
        payload: rpc_capnp::payload::Reader<'_>,
    ) -> Result<Option<alloc::boxed::Box<PlainParams>>, Error> {
        let Some(segments) = self.share_segments(messages)? else {
            return Ok(None);
        };
        let message = capnp::message::CheckedPointerReader::retain(
            payload.get_content(),
            segments,
            capnp::message::ReaderOptions {
                traversal_limit_in_words: None,
                nesting_limit: i32::MAX,
            },
        )?;
        Ok(Some(alloc::boxed::Box::new(PlainParams { message })))
    }
    /// Full serialized segment words, including unreachable data but excluding
    /// the segment table. Admission policies must not use root target_size(),
    /// which could let unused words bypass their accounting.
    #[inline(always)]
    pub fn size_in_words(
        &self,
        messages: &messages::Messages<'pool, '_, '_>,
    ) -> Result<usize, Error> {
        if let Some(length) = self.single_bytes.get() {
            return Ok((length.get() - 8) / 8);
        }
        let view = messages.read(&self.lease);
        let reader = self.reader(&view)?;
        Ok(reader.size_in_words())
    }
    /// Copy an immutable source into separate final output storage. Reading the
    /// source and writing the reserved buffer borrow independent pool cells.
    /// Failure discards only the candidate; the source remains available.
    pub fn build_from(
        &self,
        messages: &mut messages::Messages<'pool, '_, '_>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        fill: impl FnOnce(
            rpc_capnp::message::Reader<'_>,
            &mut capnp::message::FixedBuilder<'_>,
        ) -> capnp::Result<()>,
    ) -> Result<Self, Error> {
        let mut writer = messages.reserve(minimum.max(16)).map_err(Error::Storage)?;
        let result = self
            .with(messages, |source| -> Result<usize, Error> {
                let mut view = messages
                    .write_borrowed(&mut writer)
                    .map_err(Error::Storage)?;
                let length = {
                    let mut builder = capnp::message::FixedBuilder::from_bytes(&mut view[8..])?;
                    fill(source, &mut builder)?;
                    builder.into_segment().len()
                };
                let words = u32::try_from(length / 8)
                    .map_err(|_| Error::Wire(capnp::ErrorKind::MessageSizeOverflow))?;
                view[..4].copy_from_slice(&0u32.to_le_bytes());
                view[4..8].copy_from_slice(&words.to_le_bytes());
                Ok(length + 8)
            })
            .and_then(core::convert::identity);
        match result {
            Ok(length) => match messages.seal(writer, length) {
                Ok(lease) => Ok(Self {
                    lease,
                    options,
                    single_bytes: core::cell::Cell::new(
                        if options
                            .traversal_limit_in_words
                            .is_none_or(|limit| (length - 8) / 8 <= limit)
                        {
                            core::num::NonZeroUsize::new(length)
                        } else {
                            None
                        },
                    ),
                }),
                Err(rejected) => {
                    messages.discard(rejected.writer);
                    Err(Error::Storage(rejected.error))
                }
            },
            Err(error) => {
                messages.discard(writer);
                Err(error)
            }
        }
    }
    #[inline(always)]
    pub fn with<R>(
        &self,
        messages: &messages::Messages<'pool, '_, '_>,
        f: impl FnOnce(rpc_capnp::message::Reader<'_>) -> R,
    ) -> Result<R, Error> {
        let view = messages.read(&self.lease);
        let reader = self.reader(&view)?;
        reader
            .with_root(|root| root.get_as().map(f))?
            .map_err(Into::into)
    }
    /// Inspect the root together with its full serialized word charge using
    /// one framing pass. The count includes unreachable data in every segment.
    #[inline(always)]
    pub fn with_size<R>(
        &self,
        messages: &messages::Messages<'pool, '_, '_>,
        f: impl FnOnce(rpc_capnp::message::Reader<'_>, usize) -> R,
    ) -> Result<R, Error> {
        let view = messages.read(&self.lease);
        let reader = self.reader(&view)?;
        let words = self
            .single_bytes
            .get()
            .map_or_else(|| reader.size_in_words(), |length| (length.get() - 8) / 8);
        reader
            .with_root(|root| root.get_as().map(|root| f(root, words)))?
            .map_err(Into::into)
    }
}

/// Internal publication of a constructed Resolve with its exact export batch.
pub(crate) fn publish_granting<'pool, E>(
    frame: Frame<'pool>,
    batch: exports::Batch<'_, '_, '_, E>,
    messages: &mut messages::Messages<'pool, '_, '_>,
    scratch: &mut [u32],
    queue: messages::QueueSlot<'_, 'pool, '_, impl Sized>,
) -> Result<(), Error> {
    crate::publication::prepare(batch, messages, frame.lease, scratch, frame.options)
        .map(|prepared| {
            prepared.accept_into(queue);
        })
        .map_err(|error| Error::Output(error.into()))
}

#[must_use = "retry admission or release the rejected frame"]
pub struct Rejected<'pool, A> {
    pub error: Error,
    pub frame: Frame<'pool>,
    pub owner: A,
}

use crate::buffer::{Buffer, Capacity};
mod receipt_storage;
use receipt_storage::ReceiptStorage;

struct Receipts<'i, 'e, 'a, I, const N: usize> {
    // Capacity policies are admission inputs, not receipt ownership. Keeping
    // them here enlarges every queued Call/Return long after reservation ends.
    storage: ReceiptStorage<'i, 'e, 'a, I, N>,
    count: usize,
}
impl<'i, 'e, 'a, I, const N: usize> Receipts<'i, 'e, 'a, I, N> {
    fn new() -> Self {
        Self {
            storage: ReceiptStorage::new(),
            count: 0,
        }
    }
    fn initialize(
        &mut self,
        payload: rpc_capnp::payload::Reader<'_>,
        imports: &imports::Imports<'i, '_, I>,
        spare: usize,
        receiver: &mut impl ImportReceiver<I>,
        capacity: Capacity,
    ) -> Result<(), Error> {
        let descriptors = payload.get_cap_table()?;
        let count = usize::try_from(descriptors.len()).map_err(|_| Error::Capacity)?;
        self.storage
            .reserve(capacity, count.checked_add(spare).ok_or(Error::Capacity)?)?;
        // Owner factories may have side effects, so validate before calling
        // one. With no descriptors there are no factories: inbound admission
        // validates the content before committing any ledger state instead.
        if count != 0 {
            payload
                .get_content()
                .validate_capability_indices(descriptors.len())?;
        }
        self.initialize_descriptors(descriptors, count, imports, receiver, capacity)
    }
    fn initialize_descriptors<'m>(
        &mut self,
        descriptors: impl IntoIterator<Item = rpc_capnp::cap_descriptor::Reader<'m>> + Clone,
        count: usize,
        imports: &imports::Imports<'i, '_, I>,
        receiver: &mut impl ImportReceiver<I>,
        capacity: Capacity,
    ) -> Result<(), Error> {
        self.storage.reserve(capacity, count)?;
        // Validate every descriptor before invoking any owner factory.
        for descriptor in descriptors.clone() {
            match descriptor.which().map_err(|_| Error::WrongMessage)? {
                rpc_capnp::cap_descriptor::None(())
                | rpc_capnp::cap_descriptor::SenderHosted(_)
                | rpc_capnp::cap_descriptor::SenderPromise(_)
                | rpc_capnp::cap_descriptor::ReceiverHosted(_) => (),
                rpc_capnp::cap_descriptor::ThirdPartyHosted(vine) => {
                    vine?;
                }
                rpc_capnp::cap_descriptor::ReceiverAnswer(answer) => {
                    for op in answer?.get_transform()? {
                        op.which().map_err(|_| Error::WrongMessage)?;
                    }
                }
            }
        }
        let slots = self.storage.slots();
        for (index, descriptor) in descriptors.into_iter().enumerate() {
            let remote = match descriptor.which().map_err(|_| Error::WrongMessage)? {
                rpc_capnp::cap_descriptor::SenderHosted(id) => Some((id, false)),
                rpc_capnp::cap_descriptor::SenderPromise(id) => Some((id, true)),
                rpc_capnp::cap_descriptor::ThirdPartyHosted(vine) => {
                    Some((vine?.get_vine_id(), false))
                }
                rpc_capnp::cap_descriptor::ReceiverHosted(id) => {
                    slots.exports[index] = exports::LeaseSlot::new(ExportId::from_wire(id));
                    None
                }
                rpc_capnp::cap_descriptor::ReceiverAnswer(answer) => {
                    slots.answers[index] =
                        answers::PipelineSlot::new(AnswerId::from_wire(answer?.get_question_id()));
                    None
                }
                rpc_capnp::cap_descriptor::None(()) => None,
            };
            if let Some((id, promise)) = remote {
                let id = ImportId::from_wire(id);
                let alias = imports.contains(id)
                    || slots.imports[..index].iter().any(|r| r.id() == Some(id));
                slots.imports[index] = match (alias, promise) {
                    (true, false) => imports::ReceiveSlot::alias(id),
                    (true, true) => imports::ReceiveSlot::alias_promise(id),
                    (false, false) => imports::ReceiveSlot::new(id, receiver.make_import(id)),
                    (false, true) => {
                        imports::ReceiveSlot::new_promise(id, receiver.make_import(id))
                    }
                };
            }
        }
        self.count = count;
        Ok(())
    }
    // Called exactly once, after transactional admission. No decoding or fallible
    // work remains, so rejected frames never consume transport resources.
    fn deliver<'m>(
        &mut self,
        imports: &mut imports::Imports<'i, '_, I>,
        receiver: &mut impl ImportReceiver<I>,
        descriptors: impl IntoIterator<Item = rpc_capnp::cap_descriptor::Reader<'m>>,
    ) {
        for (index, descriptor) in descriptors.into_iter().enumerate() {
            if let Ok(rpc_capnp::cap_descriptor::ThirdPartyHosted(Ok(third))) = descriptor.which() {
                if let Some(local) = self.storage.imports()[index].local() {
                    receiver.introduced(imports.get_mut(local), third.get_id());
                }
            }
            let attachment = descriptor.get_attached_fd();
            if attachment != u8::MAX {
                let owner = self.storage.imports()[index]
                    .local()
                    .map(|local| imports.get_mut(local));
                receiver.attach(owner, attachment);
            }
        }
    }
    fn slots(&mut self) -> reception::full::Slots<'_, 'i, 'e, 'a, I> {
        let slots = self.storage.slots();
        reception::full::Slots {
            imports: &mut slots.imports[..self.count],
            exports: &mut slots.exports[..self.count],
            answers: &mut slots.answers[..self.count],
        }
    }
    fn release<E, A>(
        mut self,
        imports: &mut imports::Imports<'i, '_, I>,
        exports: &mut exports::Exports<'e, '_, E>,
        answers: &mut answers::Answers<'a, '_, A>,
    ) {
        if self.count != 0 {
            reception::full::release_slots(self.slots(), imports, exports, answers);
        }
    }
}

fn prepare_target_path<const N: usize>(
    target: rpc_capnp::message_target::Reader<'_>,
    fields: &mut Buffer<u16, N>,
    capacity: Capacity,
) -> Result<(), Error> {
    let count = match target.which().map_err(|_| Error::WrongMessage)? {
        rpc_capnp::message_target::ImportedCap(_) => 0,
        rpc_capnp::message_target::PromisedAnswer(answer) => {
            pipeline::Path::required(answer?).map_err(Error::Target)?
        }
    };
    if !fields.ensure(capacity, count, || 0) {
        return Err(Error::Capacity);
    }
    Ok(())
}

enum Target<'e, 'a> {
    Hosted(exports::Lease<'e>),
    Promised(answers::Pipeline<'a>),
}
pub enum TargetRef<'b, 'e, 'a> {
    Hosted(&'b exports::Lease<'e>),
    Promised {
        answer: &'b answers::Pipeline<'a>,
        path: pipeline::Path<'b>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnqueueError {
    Graph(capntproto_rpc_engine::clients::Error),
    Pipeline(pipelines::Error),
    Dispatch(capntproto_rpc_engine::clients::dispatch::DispatchError),
}
impl core::fmt::Display for EnqueueError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "owning call queue admission: {self:?}")
    }
}
impl core::error::Error for EnqueueError {}
#[must_use = "retry or release the admitted call"]
pub struct EnqueueRejected<C> {
    pub error: EnqueueError,
    pub call: C,
}

/// N and P are inline capacities for descriptor receipts (including one target)
/// and non-noop target-transform operations. `admit` uses them as hard bounds;
/// `admit_with_capacity` accepts an explicit storage policy. Rejection commits
/// no receipt or answer ownership.
#[must_use = "release the task when dispatch finishes or the call is canceled"]
pub struct Call<'pool, 'i, 'e, 'a, I, const N: usize, const P: usize> {
    frame: Frame<'pool>,
    #[cfg(feature = "alloc")]
    plain_params: core::cell::Cell<Option<alloc::boxed::Box<PlainParams>>>,
    receipts: Receipts<'i, 'e, 'a, I, N>,
    answer: answers::Key<'a>,
    target: Target<'e, 'a>,
    path: Buffer<u16, P>,
    path_len: usize,
    method: calls::DynamicCall,
    allows_handoff: bool,
}
// Admission records all ownership before the source frame is moved. This
// allows the connection classifier and Call admission to share one reader.
struct AdmittedCall<'i, 'e, 'a, I, const N: usize, const P: usize> {
    #[cfg(feature = "alloc")]
    plain_params: core::cell::Cell<Option<alloc::boxed::Box<PlainParams>>>,
    receipts: Receipts<'i, 'e, 'a, I, N>,
    answer: answers::Key<'a>,
    target: Target<'e, 'a>,
    path: Buffer<u16, P>,
    path_len: usize,
    method: calls::DynamicCall,
    allows_handoff: bool,
}
impl<'i, 'e, 'a, I, const N: usize, const P: usize> AdmittedCall<'i, 'e, 'a, I, N, P> {
    fn finish<'pool>(self, frame: Frame<'pool>) -> Call<'pool, 'i, 'e, 'a, I, N, P> {
        Call {
            frame,
            #[cfg(feature = "alloc")]
            plain_params: self.plain_params,
            receipts: self.receipts,
            answer: self.answer,
            target: self.target,
            path: self.path,
            path_len: self.path_len,
            method: self.method,
            allows_handoff: self.allows_handoff,
        }
    }
}
impl<'pool, 'i, 'e, 'a, I, const N: usize, const P: usize> Call<'pool, 'i, 'e, 'a, I, N, P> {
    /// Bind the exact admitted target and move this owning request into ordered
    /// dispatch. Queue/resource rejection returns the intact admitted request
    /// and releases any newly created pipeline dependency; retry does not admit
    /// the wire Call a second time.
    #[allow(clippy::result_large_err, clippy::type_complexity)]
    pub fn enqueue<'g, 'nodes, A, T, Peer: Copy + Eq, G, const PATH: usize>(
        self,
        runtime: &mut capntproto_rpc_engine::clients::dispatch::Runtime<
            'g,
            'nodes,
            '_,
            T,
            Peer,
            Self,
            G,
        >,
        exports: &exports::Exports<'e, '_, capntproto_rpc_engine::clients::Client<'g>>,
        answers: &mut answers::Answers<'a, '_, A>,
        bindings: &mut pipelines::Bindings<'_, 'g, 'a, PATH>,
    ) -> Result<capntproto_rpc_engine::clients::dispatch::TaskKey<'g>, EnqueueRejected<Self>>
    where
        G: capntproto_rpc_engine::clients::dispatch::GraphStorage<'g, 'nodes, T, Peer>,
    {
        self.enqueue_wrapped(
            runtime,
            exports,
            answers,
            bindings,
            |call| call,
            |call| call,
        )
    }

    #[allow(
        clippy::too_many_arguments,
        clippy::type_complexity,
        clippy::result_large_err
    )]
    pub(crate) fn enqueue_wrapped<'g, 'nodes, A, T, Peer: Copy + Eq, C, G, const PATH: usize>(
        self,
        runtime: &mut capntproto_rpc_engine::clients::dispatch::Runtime<
            'g,
            'nodes,
            '_,
            T,
            Peer,
            C,
            G,
        >,
        exports: &exports::Exports<'e, '_, capntproto_rpc_engine::clients::Client<'g>>,
        answers: &mut answers::Answers<'a, '_, A>,
        bindings: &mut pipelines::Bindings<'_, 'g, 'a, PATH>,
        wrap: impl FnOnce(Self) -> C,
        unwrap: impl FnOnce(C) -> Self,
    ) -> Result<capntproto_rpc_engine::clients::dispatch::TaskKey<'g>, EnqueueRejected<Self>>
    where
        G: capntproto_rpc_engine::clients::dispatch::GraphStorage<'g, 'nodes, T, Peer>,
    {
        let (bound, observer) = match self.target() {
            TargetRef::Hosted(lease) => (
                crate::clients::reflected(runtime, exports, lease).map_err(EnqueueError::Graph),
                false,
            ),
            TargetRef::Promised { answer, path } => (
                bindings
                    .bind(runtime, answers, answer, path)
                    .map_err(EnqueueError::Pipeline),
                true,
            ),
        };
        let client = match bound {
            Ok(client) => client,
            Err(error) => return Err(EnqueueRejected { error, call: self }),
        };
        let key = client.key();
        let mut call = Some(wrap(self));
        let outcome = runtime
            .prepare_call(&client, &mut call)
            .map(|pending| pending.accept());
        runtime.release(client);
        match outcome {
            Ok(task) => Ok(task),
            Err(error) => {
                if observer {
                    bindings.discard_unexposed(key, answers);
                }
                Err(EnqueueRejected {
                    error: EnqueueError::Dispatch(error),
                    call: unwrap(call.expect("unaccepted request owner")),
                })
            }
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub fn admit<E, A>(
        frame: Frame<'pool>,
        messages: &messages::Messages<'pool, '_, '_>,
        imports: &mut imports::Imports<'i, '_, I>,
        exports: &mut exports::Exports<'e, '_, E>,
        answers: &mut answers::Answers<'a, '_, A>,
        owner: A,
        make_import: impl FnMut(ImportId) -> I,
    ) -> Result<Self, Rejected<'pool, A>> {
        Self::admit_with_capacity(
            frame,
            messages,
            imports,
            exports,
            answers,
            owner,
            make_import,
            Capacity::Inline,
            Capacity::Inline,
        )
    }
    #[allow(clippy::too_many_arguments)]
    pub fn admit_with_capacity<E, A>(
        frame: Frame<'pool>,
        messages: &messages::Messages<'pool, '_, '_>,
        imports: &mut imports::Imports<'i, '_, I>,
        exports: &mut exports::Exports<'e, '_, E>,
        answers: &mut answers::Answers<'a, '_, A>,
        owner: A,
        mut make_import: impl FnMut(ImportId) -> I,
        capacity: Capacity,
        path_capacity: Capacity,
    ) -> Result<Self, Rejected<'pool, A>> {
        Self::admit_with_receiver(
            frame,
            messages,
            imports,
            exports,
            answers,
            owner,
            &mut make_import,
            capacity,
            path_capacity,
        )
    }
    #[allow(clippy::too_many_arguments, clippy::result_large_err)]
    pub fn admit_with_receiver<E, A>(
        frame: Frame<'pool>,
        messages: &messages::Messages<'pool, '_, '_>,
        imports: &mut imports::Imports<'i, '_, I>,
        exports: &mut exports::Exports<'e, '_, E>,
        answers: &mut answers::Answers<'a, '_, A>,
        owner: A,
        receiver: &mut impl ImportReceiver<I>,
        capacity: Capacity,
        path_capacity: Capacity,
    ) -> Result<Self, Rejected<'pool, A>> {
        let mut owner = Some(owner);
        let outcome = frame
            .with(messages, |root| {
                let rpc_capnp::message::Call(call) =
                    root.which().map_err(|_| Error::WrongMessage)?
                else {
                    return Err(Error::WrongMessage);
                };
                Self::admit_decoded(
                    &frame,
                    messages,
                    call?,
                    imports,
                    exports,
                    answers,
                    &mut owner,
                    receiver,
                    capacity,
                    path_capacity,
                )
            })
            .and_then(core::convert::identity);
        match outcome {
            Ok(admitted) => Ok(admitted.finish(frame)),
            Err(error) => Err(Rejected {
                error,
                frame,
                owner: owner.unwrap(),
            }),
        }
    }
    #[allow(clippy::too_many_arguments)]
    #[inline(never)]
    fn admit_decoded<E, A>(
        _frame: &Frame<'pool>,
        _messages: &messages::Messages<'pool, '_, '_>,
        call: rpc_capnp::call::Reader<'_>,
        imports: &mut imports::Imports<'i, '_, I>,
        exports: &mut exports::Exports<'e, '_, E>,
        answers: &mut answers::Answers<'a, '_, A>,
        owner: &mut Option<A>,
        receiver: &mut impl ImportReceiver<I>,
        capacity: Capacity,
        path_capacity: Capacity,
    ) -> Result<AdmittedCall<'i, 'e, 'a, I, N, P>, Error> {
        let mut receipts = Receipts::new();
        #[cfg(feature = "alloc")]
        let mut plain_params = None;
        let mut path = Buffer::Inline([0; P]);
        let (answer, target, path_len, method, allows_handoff) = (|| {
            let target = call.get_target()?;
            let payload = call.get_params()?;
            let descriptors = payload.get_cap_table()?;
            // The target receipt is moved into Call::target at acceptance;
            // it does not need persistent payload storage. In particular,
            // a descriptor-free hosted call (N = 0) must not allocate three
            // vectors and their container solely for that temporary slot.
            // Still charge the target against the configured receipt bound.
            let plain = descriptors.is_empty();
            if plain
                && matches!(
                    call.get_send_results_to().which(),
                    Ok(rpc_capnp::call::send_results_to::Caller(()))
                )
            {
                if let Ok(rpc_capnp::message_target::ImportedCap(id)) = target.which() {
                    // There are no payload receipts or pipeline dependencies.
                    // Preflight the same two ledgers directly, retaining both
                    // exclusive tickets until every fallible operation ends.
                    if capacity.maximum::<N>() == 0 {
                        return Err(Error::Capacity);
                    }
                    inbound::caller_question(call.get_question_id()).map_err(Error::Admission)?;
                    payload
                        .get_content()
                        .validate_capability_indices(0)
                        .map_err(|e| Error::Admission(inbound::Error::Reception(e.into())))?;
                    let target = exports
                        .prepare_lease(ExportId::from_wire(id))
                        .map_err(|e| {
                            Error::Admission(inbound::Error::Reception(reception::Error::Exports(
                                e.into(),
                            )))
                        })?;
                    let answer = answers
                        .prepare_admission(AnswerId::from_wire(call.get_question_id()), None)
                        .map_err(|e| {
                            Error::Admission(inbound::Error::Reception(reception::Error::Answers(
                                e.into(),
                            )))
                        })?
                        .allow_handoff(call.get_allow_third_party_tail_call());
                    #[cfg(feature = "alloc")]
                    {
                        plain_params = _frame.retain_plain_payload(_messages, payload)?;
                    }
                    let target = Target::Hosted(target.accept());
                    let (answer, dependency) = answer.accept(owner.take().unwrap());
                    debug_assert!(dependency.is_none());
                    return Ok((
                        answer,
                        target,
                        0,
                        calls::Incoming::new(call).hints(),
                        call.get_allow_third_party_tail_call(),
                    ));
                }
            }
            prepare_target_path(target, &mut path, path_capacity)?;
            if plain {
                if capacity.maximum::<N>() == 0 {
                    return Err(Error::Capacity);
                }
            } else {
                receipts.initialize(payload, imports, 1, receiver, capacity)?;
            }
            let count = receipts.count;
            let mut target_export = [exports::LeaseSlot::none()];
            let mut target_answer = [answers::PipelineSlot::none()];
            let slots = if plain {
                reception::full::Slots {
                    imports: &mut [],
                    exports: &mut target_export,
                    answers: &mut target_answer,
                }
            } else {
                receipts.storage.slots()
            };
            match target.which().map_err(|_| Error::WrongMessage)? {
                rpc_capnp::message_target::ImportedCap(id) => {
                    slots.exports[count] = exports::LeaseSlot::new(ExportId::from_wire(id))
                }
                rpc_capnp::message_target::PromisedAnswer(answer) => {
                    slots.answers[count] =
                        answers::PipelineSlot::new(AnswerId::from_wire(answer?.get_question_id()))
                }
            }
            let pending = inbound::full::call_parts(
                call,
                target,
                payload,
                imports,
                exports,
                answers,
                reception::full::Slots {
                    imports: &mut slots.imports[..count],
                    exports: &mut slots.exports[..count + 1],
                    answers: &mut slots.answers[..count + 1],
                },
                &mut path,
            )
            .map_err(Error::Admission)?;
            // Retain the checked payload while its admission reader is live.
            // Fixed storage returns None and never allocates. All fallible
            // retention is before committing answer/receipt ownership.
            #[cfg(feature = "alloc")]
            if plain {
                plain_params = _frame.retain_plain_payload(_messages, payload)?;
            }
            let admitted = pending.accept(owner.take().unwrap());
            let (target, length) = match admitted.target {
                pipeline::Target::Hosted(lease) => (Target::Hosted(lease), 0),
                pipeline::Target::Promised { answer, path } => {
                    (Target::Promised(answer), path.fields().len())
                }
            };
            let answer = admitted.answer;
            receipts.deliver(imports, receiver, descriptors);
            let incoming = calls::Incoming::new(call);
            Ok((
                answer,
                target,
                length,
                incoming.hints(),
                incoming.allows_third_party_tail_call(),
            ))
        })()?;
        Ok(AdmittedCall {
            #[cfg(feature = "alloc")]
            plain_params: core::cell::Cell::new(plain_params),
            receipts,
            answer,
            target,
            path,
            path_len,
            method,
            allows_handoff,
        })
    }
    pub fn answer(&self) -> answers::Key<'a> {
        self.answer
    }
    /// Scalar method metadata captured from the immutable admitted Call.
    pub fn method(&self) -> calls::DynamicCall {
        self.method
    }
    /// Whether this admitted ordinary Call permits third-party answer handoff.
    pub fn allows_third_party_tail_call(&self) -> bool {
        self.allows_handoff
    }
    pub fn target(&self) -> TargetRef<'_, 'e, 'a> {
        match &self.target {
            Target::Hosted(lease) => TargetRef::Hosted(lease),
            Target::Promised(answer) => TargetRef::Promised {
                answer,
                path: pipeline::Path::new(&self.path[..self.path_len]),
            },
        }
    }
    pub fn with<R>(
        &self,
        messages: &messages::Messages<'pool, '_, '_>,
        f: impl FnOnce(Request<'_, '_, 'i, 'e, 'a, I, N>) -> R,
    ) -> Result<R, Error> {
        self.frame.with(messages, |root| {
            let rpc_capnp::message::Call(call) = root.which().map_err(|_| Error::WrongMessage)?
            else {
                return Err(Error::WrongMessage);
            };
            Ok(f(Request {
                incoming: calls::Incoming::new(call?),
                receipts: &self.receipts,
            }))
        })?
    }
    /// Retain capability-free parameters with their already admitted segment
    /// layout. The selected reader owns the same immutable bytes; fixed caller
    /// storage and capability-bearing requests use the receipt-based path.
    #[cfg(feature = "alloc")]
    pub fn share_plain_params(
        &self,
        messages: &messages::Messages<'pool, '_, '_>,
    ) -> Result<Option<alloc::boxed::Box<PlainParams>>, Error> {
        if let Some(params) = self.plain_params.take() {
            return Ok(Some(params));
        }
        if self.receipts.count != 0 {
            return Ok(None);
        }
        let Some(segments) = self.frame.share_segments(messages)? else {
            return Ok(None);
        };
        self.with(messages, |params| {
            let payload = params.incoming().payload()?;
            capnp::message::CheckedPointerReader::retain(
                payload.get_content(),
                segments,
                capnp::message::ReaderOptions {
                    traversal_limit_in_words: None,
                    nesting_limit: i32::MAX,
                },
            )
            .map(|message| Some(alloc::boxed::Box::new(PlainParams { message })))
        })?
        .map_err(Error::from)
    }
    /// Share the admitted frame's immutable bytes, without interpreting IDs as
    /// capability authority. Its receipts remain available only through `with`.
    #[cfg(feature = "alloc")]
    pub fn share_frame(
        &self,
        messages: &messages::Messages<'pool, '_, '_>,
    ) -> Option<messages::SharedBytes> {
        self.frame.share(messages)
    }
    /// The future borrows only this task and its reserved bytes, not the message
    /// pool or connection tables. Other frames can progress during suspension.
    pub fn with_async<'call, 'storage: 'call, R, F>(
        &'call self,
        messages: &messages::Messages<'pool, 'storage, '_>,
        f: F,
    ) -> impl core::future::Future<Output = Result<R, Error>>
           + 'call
           + use<'call, 'pool, 'i, 'e, 'a, I, R, F, N, P>
    where
        F: for<'m> core::ops::AsyncFnOnce(Request<'m, 'call, 'i, 'e, 'a, I, N>) -> R + 'call,
    {
        let view = messages.read(&self.frame.lease);
        async move {
            let mut bytes = &*view;
            let reader = capnp::serialize::read_message_from_flat_slice_no_alloc(
                &mut bytes,
                self.frame.options,
            )?;
            let root = reader.get_root::<rpc_capnp::message::Reader<'_>>()?;
            let rpc_capnp::message::Call(call) = root.which().map_err(|_| Error::WrongMessage)?
            else {
                return Err(Error::WrongMessage);
            };
            Ok(f(Request {
                incoming: calls::Incoming::new(call?),
                receipts: &self.receipts,
            })
            .await)
        }
    }
    pub fn release<E, A>(
        self,
        messages: &mut messages::Messages<'pool, '_, '_>,
        imports: &mut imports::Imports<'i, '_, I>,
        exports: &mut exports::Exports<'e, '_, E>,
        answers: &mut answers::Answers<'a, '_, A>,
    ) {
        self.receipts.release(imports, exports, answers);
        match self.target {
            Target::Hosted(lease) => exports.release_local_deferred(lease),
            Target::Promised(pipeline) => answers.release_pipeline_deferred(pipeline),
        }
        self.frame.release(messages);
    }
}

pub struct Request<'m, 'r, 'i, 'e, 'a, I, const N: usize> {
    incoming: calls::Incoming<'m>,
    receipts: &'r Receipts<'i, 'e, 'a, I, N>,
}
impl<'m, 'r, 'i, 'e, 'a, I, const N: usize> Request<'m, 'r, 'i, 'e, 'a, I, N> {
    pub fn capability_count(&self) -> usize {
        self.receipts.count
    }
    pub fn capability_at(
        &self,
        index: u32,
    ) -> Result<Option<reception::full::Capability<'r, 'm, 'i, 'e, 'a>>, reception::Error> {
        let i = index as usize;
        if i >= self.receipts.count {
            return Err(reception::Error::MissingCapability);
        }
        let descriptor = self.incoming.payload()?.get_cap_table()?.get(index);
        if matches!(descriptor.which(), Ok(rpc_capnp::cap_descriptor::None(()))) {
            return Ok(None);
        }
        reception::full::descriptor_capability(
            descriptor,
            &self.receipts.storage.imports()[i],
            &self.receipts.storage.exports()[i],
            &self.receipts.storage.answers()[i],
        )
        .map(Some)
    }
    pub fn incoming(&self) -> &calls::Incoming<'m> {
        &self.incoming
    }
    pub fn capability<T>(
        &self,
        pointer: capnp::wire_capability::Reader<'_, T>,
    ) -> Result<reception::full::Capability<'r, 'm, 'i, 'e, 'a>, reception::Error> {
        let payload = self.incoming.payload()?;
        if !pointer.belongs_to(payload.get_content()) {
            return Err(reception::Error::ForeignMessage);
        }
        let index =
            usize::try_from(pointer.index()).map_err(|_| reception::Error::MissingCapability)?;
        if index >= self.receipts.count {
            return Err(reception::Error::MissingCapability);
        }
        reception::full::descriptor_capability(
            payload.get_cap_table()?.get(pointer.index()),
            &self.receipts.storage.imports()[index],
            &self.receipts.storage.exports()[index],
            &self.receipts.storage.answers()[index],
        )
    }
}

/// Retained admitted parameters with no capability descriptors. Construction
/// is private so unchecked frames cannot bypass admission's traversal limits.
#[cfg(feature = "alloc")]
pub struct PlainParams {
    message: capnp::message::CheckedPointerReader<
        capnp::serialize::NoAllocBufferSegments<messages::SharedBytes>,
    >,
}
#[cfg(feature = "alloc")]
impl capnp::private::capability::ParamsHook for PlainParams {
    fn get(&self) -> capnp::Result<capnp::any_pointer::Reader<'_>> {
        Ok(self.message.root())
    }
}

#[cfg(test)]
mod attachment_tests;
#[cfg(test)]
mod tests;

mod question;
#[cfg(feature = "alloc")]
pub use question::PlainResults;
pub use question::{IncomingAdoption, Question, Reply, Response, ReturnRejected};

pub mod answer;
pub mod connection;
pub mod controls;
pub mod pipelines;
mod resolution;
pub use resolution::{ApplyError, ApplyRejected, Resolution, ResolutionView};
pub mod tasks;

pub mod loopback;
