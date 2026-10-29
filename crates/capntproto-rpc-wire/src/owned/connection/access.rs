//! Restricted mutation scopes and authoritative ledger transactions.
// Rejections return their affine input owners without allocating in fixed storage.
#![allow(clippy::result_large_err)]

use super::*;

impl<'pool, 'id, 's, I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize>
    Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>
{
    /// Read-only dispatcher inspection carries no mutation authority.
    pub fn runtime(&self) -> &Runtime<'pool, 'id, 's, I, P, S, CAPS, PATH> {
        &self.runtime
    }
    /// Transitions emit their exact readiness effects inside the dispatcher.
    pub fn capabilities(&mut self) -> Scope<'_, 'pool, 'id, 's, I, P, S, CAPS, PATH> {
        clients::Access::with_export_sources(&mut self.runtime, Some(&self.ledgers.exports))
    }
    /// Inspect authoritative import metadata while editing capability bindings.
    #[allow(clippy::type_complexity)]
    pub fn split_graph(
        &mut self,
    ) -> (
        Scope<'_, 'pool, 'id, 's, I, P, S, CAPS, PATH>,
        &connection::Connection<'id, 's, I, clients::Client<'id>, Answer<'pool, 'id, A, CAPS>>,
    ) {
        (
            clients::Access::with_export_sources(&mut self.runtime, Some(&self.ledgers.exports)),
            &self.ledgers,
        )
    }
}

impl<'pool, 'id, 's, I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize>
    Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>
{
    pub fn execution(
        &mut self,
    ) -> (
        Scope<'_, 'pool, 'id, 's, I, P, S, CAPS, PATH>,
        answers::Execution<'_, 'id, 's, Answer<'pool, 'id, A, CAPS>>,
    ) {
        (
            clients::Access::with_export_sources(&mut self.runtime, Some(&self.ledgers.exports)),
            answers::Execution::new(&mut self.ledgers.answers),
        )
    }
    pub fn bind_capability(
        &mut self,
        cap: crate::reception::full::Capability<'_, '_, 'id, 'id, 'id>,
    ) -> Result<binding::Bound<'id>, binding::BindError> {
        self.imports.bind_capability(
            cap,
            &mut self.runtime,
            &mut self.ledgers.imports,
            &self.ledgers.exports,
            &mut self.ledgers.answers,
            &mut self.pipelines,
        )
    }
    pub fn bind_import(
        &mut self,
        local: &imports::Local<'id>,
    ) -> Result<clients::Client<'id>, binding::Error> {
        self.imports
            .bind(&mut self.runtime, &mut self.ledgers.imports, local)
    }
    pub fn reflected(
        &mut self,
        lease: &exports::Lease<'id>,
    ) -> Result<clients::Client<'id>, clients::Error> {
        binding::reflected(&mut self.runtime, &self.ledgers.exports, lease)
    }
    pub fn project_answer(
        &mut self,
        answer: &answers::Pipeline<'id>,
        path: pipeline::Path<'_>,
    ) -> Result<clients::Client<'id>, pipelines::Error> {
        self.pipelines
            .bind(&mut self.runtime, &mut self.ledgers.answers, answer, path)
    }
}

impl<'pool, 'id, 's, I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize>
    Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>
{
    /// Shared inspection cannot replace ledgers or edit notification ownership.
    pub fn ledgers(
        &self,
    ) -> &connection::Connection<'id, 's, I, clients::Client<'id>, Answer<'pool, 'id, A, CAPS>>
    {
        &self.ledgers
    }
    pub fn answers(&mut self) -> answers::Execution<'_, 'id, 's, Answer<'pool, 'id, A, CAPS>> {
        answers::Execution::new(&mut self.ledgers.answers)
    }
    pub fn prepare_answer_return(
        &mut self,
        key: answers::Key<'id>,
    ) -> Result<answers::Return<'_, 'id, 's, Answer<'pool, 'id, A, CAPS>>, answers::Error> {
        self.ledgers.answers.prepare_return(key)
    }
    pub fn answer_pipeline(
        &mut self,
        id: AnswerId,
    ) -> Result<answers::Pipeline<'id>, answers::Error> {
        self.ledgers.answers.pipeline(id)
    }
    pub fn release_answer_pipeline(
        &mut self,
        lease: answers::Pipeline<'id>,
    ) -> Option<Answer<'pool, 'id, A, CAPS>> {
        self.ledgers.answers.release_pipeline(lease)
    }
    pub fn release_answer_pipeline_deferred(&mut self, lease: answers::Pipeline<'id>) {
        self.ledgers.answers.release_pipeline_deferred(lease)
    }
    pub fn admit_answer(
        &mut self,
        id: AnswerId,
        value: Answer<'pool, 'id, A, CAPS>,
    ) -> Result<answers::Key<'id>, answers::Rejected<Answer<'pool, 'id, A, CAPS>>> {
        self.ledgers.answers.admit(id, value)
    }
    pub fn finish_answer(
        &mut self,
        id: AnswerId,
        release: bool,
    ) -> answers::Finished<Answer<'pool, 'id, A, CAPS>> {
        self.ledgers.answers.finish(id, release)
    }
    #[cfg(feature = "alloc")]
    pub fn retain_call_credit(
        &mut self,
        key: questions::Key<'id>,
        credit: alloc::rc::Rc<capntproto_rpc_engine::flow_control::outgoing::OwnedPermit>,
    ) -> Result<(), questions::Error> {
        self.ledgers.questions.retain_credit(key, credit)
    }
    pub fn prepare_imports<'table, 'receipts>(
        &'table mut self,
        receipts: &'receipts mut [imports::ReceiveSlot<'id, I>],
    ) -> Result<imports::Receive<'table, 'receipts, 'id, 's, I>, imports::ReceiveError> {
        self.ledgers.imports.prepare_receive(receipts)
    }
    pub fn release_import(&mut self, local: imports::Local<'id>) {
        self.ledgers.imports.release(local)
    }
    pub fn retain_import(
        &mut self,
        local: &imports::Local<'id>,
    ) -> Result<imports::Local<'id>, imports::Error> {
        self.ledgers.imports.retain(local)
    }
    pub fn admit_import(
        &mut self,
        id: ImportId,
        value: I,
    ) -> Result<imports::Local<'id>, imports::Rejected<I>> {
        self.ledgers.imports.admit(id, value)
    }
    pub fn release_export(
        &mut self,
        id: ExportId,
        count: u32,
    ) -> Result<Option<clients::Client<'id>>, exports::Error> {
        self.ledgers.exports.release(id, count)
    }
    /// A publication transaction and its restricted graph scope share a borrow.
    /// The batch still commits only after output accepts its exact descriptors.
    pub fn export_batch(
        &mut self,
    ) -> (
        Scope<'_, 'pool, 'id, 's, I, P, S, CAPS, PATH>,
        exports::Batch<'_, 'id, 's, clients::Client<'id>>,
    ) {
        (
            clients::Access::with_export_sources(&mut self.runtime, None),
            self.ledgers.exports.batch(),
        )
    }
    #[cfg(feature = "alloc")]
    pub fn observe_counts(&mut self) -> [capntproto_rpc_engine::observation::Observation; 4] {
        [
            self.ledgers.questions.observe_len(),
            self.ledgers.answers.observe_len(),
            self.ledgers.imports.observe_len(),
            self.ledgers.exports.observe_len(),
        ]
    }
}

impl<'pool, 'id, 's, I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize>
    Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>
{
    /// Admit a caller-executed request through the same authoritative ledgers.
    /// The returned affine owner must be released through this driver.
    pub fn admit_request(
        &mut self,
        frame: Frame<'pool>,
        messages: &messages::Messages<'pool, '_, '_>,
        owner: Answer<'pool, 'id, A, CAPS>,
        make_import: impl FnMut(ImportId) -> I,
    ) -> Result<
        Request<'pool, 'id, I, CAPS, PATH>,
        crate::owned::Rejected<'pool, Answer<'pool, 'id, A, CAPS>>,
    > {
        Request::admit_with_capacity(
            frame,
            messages,
            &mut self.ledgers.imports,
            &mut self.ledgers.exports,
            &mut self.ledgers.answers,
            owner,
            make_import,
            self.message_capabilities,
            self.message_path,
        )
    }
    pub fn prepare_answer_adoption(
        &mut self,
        id: AnswerId,
    ) -> Result<answers::Adoption<'_, 'id, 's, Answer<'pool, 'id, A, CAPS>>, answers::Error> {
        self.ledgers.answers.prepare_adoption(id)
    }
    pub fn reserve_question_adoption(
        &mut self,
        id: capntproto_rpc_engine::ids::QuestionId,
    ) -> Result<questions::AdoptionReservation<'_, 'id, 's>, questions::Error> {
        self.ledgers.questions.reserve_adoption(id)
    }
}

impl<'pool, 'id, 's, I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize>
    Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>
{
    pub fn release_answer(
        &mut self,
        answer: Answer<'pool, 'id, A, CAPS>,
        messages: &mut messages::Messages<'pool, '_, '_>,
    ) -> A {
        answer
            .release(messages)
            .release(messages, &mut self.runtime, |child| {
                self.ledgers.questions.release(child)
            })
    }
    pub fn send_pending_finish(
        &mut self,
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, impl Default>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
    ) -> Result<bool, crate::owned::Error> {
        controls::finish_one(
            &mut self.ledgers.questions,
            messages,
            queue,
            minimum,
            options,
        )
    }
    pub fn send_pending_release(
        &mut self,
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, impl Default>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
    ) -> Result<Option<I>, crate::owned::Error> {
        controls::release_one(&mut self.ledgers.imports, messages, queue, minimum, options)
    }
    pub fn publish_answer_frame(
        &mut self,
        key: answers::Key<'id>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        frame: Frame<'pool>,
        slot: messages::QueueSlot<'_, 'pool, '_, impl Sized>,
    ) -> Result<Option<Answer<'pool, 'id, A, CAPS>>, crate::owned::Error> {
        answer::send(key, &mut self.ledgers.answers, messages, frame, slot)
    }
    pub fn try_publish_answer_frame(
        &mut self,
        key: answers::Key<'id>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        frame: Frame<'pool>,
        slot: messages::QueueSlot<'_, 'pool, '_, impl Sized>,
    ) -> Result<Option<Answer<'pool, 'id, A, CAPS>>, answer::Rejected<'pool>> {
        answer::try_send(key, &mut self.ledgers.answers, messages, frame, slot)
    }
    pub fn publish_answer_pipeline(
        &mut self,
        key: answers::Key<'id>,
        result: Result<pipelines::Published<'pool, 'id, CAPS>, capnp::ErrorKind>,
    ) -> Result<(), pipelines::CompletionRejected<'pool, 'id, CAPS>> {
        pipelines::complete(&mut self.ledgers.answers, key, result)
    }
    pub fn freeze_import(
        &mut self,
        local: &imports::Local<'id>,
    ) -> Result<clients::Client<'id>, binding::Error> {
        self.imports
            .freeze_import(&mut self.runtime, &mut self.ledgers.imports, local)
    }
    /// Freeze a descriptor table consisting entirely of retained reflected
    /// routes. This operation grants no exports and consumes no receipts.
    pub fn snapshot_reflected_results(
        &mut self,
        frame: &Frame<'pool>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        references: &mut [crate::publication::reflected::Reference<'id, 'id, '_>],
        scratch: &mut [u32],
    ) -> Result<pipelines::Published<'pool, 'id, CAPS>, pipelines::Error> {
        let mut references = crate::publication::reflected::References {
            imports: &mut self.ledgers.imports,
            questions: &mut self.ledgers.questions,
            slots: references,
        };
        pipelines::Published::from_reflected_with_capacity(
            frame,
            messages,
            &self.ledgers.exports.batch(),
            &mut self.runtime,
            &self.imports,
            &mut references,
            scratch,
            self.message_capabilities,
            self.message_path,
        )
    }
}

impl<'pool, 'id, 's, I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize>
    Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>
{
    pub fn pending_import_release(&mut self, index: usize) -> Option<imports::Release<'_, I>> {
        self.ledgers.imports.pending_release(index)
    }
}
