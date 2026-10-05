// Copyright (c) 2013-2015 Sandstorm Development Group, Inc. and contributors
// Licensed under the MIT License:
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

//! Capability descriptors, import/export resolution and third-party introductions.
use super::*;

impl<VatId: 'static> ConnectionState<VatId> {
    pub(super) fn get_message_target(
        &self,
        target: message_target::Reader,
    ) -> ::capnp::Result<Box<dyn ClientHook>> {
        match target.which()? {
            message_target::ImportedCap(export_id) => {
                match self.exports.borrow().get(ExportId::from_wire(export_id)) {
                    Some(exp) => Ok(exp.client_hook.clone()),
                    _ => Err(Error::failed(
                        "Message target is not a current export ID.".to_string(),
                    )),
                }
            }
            message_target::PromisedAnswer(promised_answer) => {
                let promised_answer = promised_answer?;
                let question_id = AnswerId::from_wire(promised_answer.get_question_id());

                let pipeline = match self.answers.borrow().slots.get(&question_id) {
                    None => Box::new(broken::Pipeline::new(Error::failed(
                        "Pipeline call on a request that returned no capabilities or was already closed.".to_string(),
                    ))) as Box<dyn PipelineHook>,
                    Some(base) => {
                        match base.pipeline {
                            Some(ref pipeline) => pipeline.add_ref(),
                            None => Box::new(broken::Pipeline::new(Error::failed(
                                "Pipeline call on a request that returned not capabilities or was \
                                 already closed."
                                    .to_string(),
                            ))) as Box<dyn PipelineHook>,
                        }
                    }
                };
                let ops = to_pipeline_ops(promised_answer.get_transform()?)?;
                Ok(pipeline.get_pipelined_cap(&ops))
            }
        }
    }

    /// If calls to the given capability should pass over this connection, fill in `target`
    /// appropriately for such a call and return None. Otherwise, return a `ClientHook` to which
    /// the call should be forwarded; the caller should then delegate the call to that `ClientHook`.
    ///
    /// The main case where this ends up returning Some(_) is if `cap` is a promise that has
    /// recently resolved. The application might have started building a request before the promise
    /// resolved, and so the request may have been built on the assumption that it would be sent over
    /// this network connection, but then the promise resolved to point somewhere else before the
    /// request was sent. Now the request has to be redirected to the new target instead.
    pub(super) fn write_target(
        &self,
        cap: &dyn ClientHook,
        target: message_target::Builder,
    ) -> Option<Box<dyn ClientHook>> {
        if cap.get_brand() == self.get_brand() {
            match Client::from_ptr(cap.get_ptr(), self) {
                Some(c) => c.write_target(target),
                None => unreachable!(),
            }
        } else {
            Some(cap.add_ref())
        }
    }

    /// If the given client just wraps some other client -- even if it is only *temporarily*
    /// wrapping that other client -- returns a reference to the other client, transitively.
    /// Otherwise, returns a new reference to *this.
    pub(super) fn get_innermost_client(
        &self,
        mut client: Box<dyn ClientHook>,
    ) -> Box<dyn ClientHook> {
        while let Some(inner) = client.get_resolved() {
            client = inner;
        }
        if client.get_brand() == self.get_brand() {
            match self.client_downcast_map.borrow().get(&client.get_ptr()) {
                Some(c) => Box::new(c.upgrade().expect("dangling client?")),
                None => unreachable!(),
            }
        } else {
            client
        }
    }

    /// Implements exporting of a promise.  The promise has been exported under the given ID, and is
    /// to eventually resolve to the ClientHook produced by `promise`.  This method waits for that
    /// resolve to happen and then sends the appropriate `Resolve` message to the peer.
    #[allow(clippy::await_holding_refcell_ref)] // https://github.com/rust-lang/rust-clippy/issues/6353
    pub(super) fn resolve_exported_promise(
        state: &Rc<Self>,
        export_id: ExportId,
        promise: Promise<Box<dyn ClientHook>, Error>,
    ) -> Promise<(), Error> {
        let weak_connection_state = Rc::downgrade(state);
        state.eagerly_evaluate(Promise::from_future(async move {
            let resolution_result = promise.await;
            let connection_state = weak_connection_state
                .upgrade()
                .expect("dangling connection state?");

            match resolution_result {
                Ok(resolution) => {
                    let resolution = connection_state.get_innermost_client(resolution.clone());

                    let brand = resolution.get_brand();

                    // Update the export table to point at this object instead. We know that our
                    // entry in the export table is still live because when it is destroyed the
                    // asynchronous resolution task (i.e. this code) is canceled.
                    let mut exports = connection_state.exports.borrow_mut();
                    let Some(exp) = exports.find(export_id) else {
                        return Err(Error::failed("export table entry not found".to_string()));
                    };

                    if exp.canonical {
                        connection_state
                            .exports_by_cap
                            .borrow_mut()
                            .remove(&exp.client_hook.get_ptr());
                    }
                    exp.client_hook = resolution.clone();

                    // The export now points to `resolution`, but it is not necessarily the
                    // canonical export for `resolution`. The export itself still represents
                    // the promise that ended up resolving to `resolution`, but `resolution`
                    // itself also needs to be exported under a separate export ID to
                    // distinguish from the promise. (Unless it's also a promise, see the next
                    // bit...)
                    exp.canonical = false;

                    if brand != connection_state.get_brand() {
                        // We're resolving to a local capability. If we're resolving to a promise,
                        // we might be able to reuse our export table entry and avoid sending a
                        // message.
                        if let Some(promise) = resolution.when_more_resolved() {
                            // We're replacing a promise with another local promise. In this case,
                            // we might actually be able to just reuse the existing export table
                            // entry to represent the new promise -- unless it already has an entry.
                            // Let's check.

                            let mut exports_by_cap = connection_state.exports_by_cap.borrow_mut();

                            let replacement_export_id =
                                match exports_by_cap.entry(exp.client_hook.get_ptr()) {
                                    hash_map::Entry::Occupied(occ) => *occ.get(),
                                    hash_map::Entry::Vacant(vac) => {
                                        // The replacement capability isn't previously exported,
                                        // so assign it to the existing table entry.
                                        vac.insert(export_id);
                                        export_id
                                    }
                                };
                            if replacement_export_id == export_id {
                                // The new promise was not already in the table, therefore the existing
                                // export table entry has now been repurposed to represent it. There is
                                // no need to send a resolve message at all. We do, however, have to
                                // start resolving the next promise.
                                exp.canonical = true;
                                drop(exports);
                                drop(exports_by_cap);
                                return Self::resolve_exported_promise(
                                    &connection_state,
                                    export_id,
                                    promise,
                                )
                                .await;
                            }
                        }
                    }
                    // Prevent a double borrow in write_descriptor() below.
                    drop(exports);

                    // OK, we have to send a `Resolve` message.
                    let mut fds = OutgoingFds::default();
                    let mut message = connection_state.new_outgoing_message(15)?;
                    {
                        let root: message::Builder = message.get_body()?.get_as()?;
                        let mut resolve = root.init_resolve();
                        resolve.set_promise_id(export_id.to_wire());
                        let _export = Self::write_descriptor(
                            &connection_state,
                            resolution,
                            resolve.init_cap(),
                            &mut fds,
                        )?;
                    }
                    fds.attach(&mut *message);
                    let _ = message.send_detached();
                    Ok(())
                }
                Err(e) => {
                    // send error resolution
                    let mut message = connection_state.new_outgoing_message(15)?;
                    {
                        let root: message::Builder = message.get_body()?.get_as()?;
                        let mut resolve = root.init_resolve();
                        resolve.set_promise_id(export_id.to_wire());
                        from_error(
                            &e,
                            resolve.init_exception(),
                            connection_state.encode_trace(&e).as_deref(),
                        );
                    }
                    let _ = message.send_detached();
                    Ok(())
                }
            }
        }))
    }

    pub(super) fn try_introduce(
        state: &Rc<Self>,
        inner: &dyn ClientHook,
        mut descriptor: cap_descriptor::Builder<'_>,
    ) -> capnp::Result<Option<ExportId>> {
        let Some(registry) = state.registry.upgrade() else {
            return Ok(None);
        };
        let host = registry
            .borrow()
            .states
            .values()
            .find(|c| c.get_brand() == inner.get_brand())
            .cloned();
        let Some(host) = host else {
            return Ok(None);
        };
        // Deferred introductions must be inspected before when_more_resolved(),
        // which is a request to accept them locally.
        if let Some(Client {
            variant: ClientVariant::ThirdParty(third),
            ..
        }) = Client::from_ptr(inner.get_ptr(), &host)
        {
            if let Some(export) = third.forward_to(state, descriptor.reborrow())? {
                return Ok(Some(export));
            }
            let accepted = third.ensure_accepted();
            return Self::try_introduce(state, &*accepted, descriptor);
        }
        // Never introduce an unresolved promise. Its eventual Resolve will use
        // this same path, after the original route has been recorded.
        if inner.when_more_resolved().is_some() {
            return Ok(None);
        }
        let mut recipient = capnp::message::Builder::new_default();
        let mut contact = capnp::message::Builder::new_default();
        let peer = state
            .connection
            .borrow()
            .as_ref()
            .map_err(|e| e.clone())?
            .get_peer_vat_id();
        let introduced = host
            .connection
            .borrow_mut()
            .as_mut()
            .map_err(|e| e.clone())?
            .introduce_to(peer, contact.init_root(), recipient.init_root())?;
        if !introduced {
            return Ok(None);
        }
        let mut message = host.new_outgoing_message(32)?;
        {
            let mut provide = message
                .get_body()?
                .init_as::<message::Builder>()
                .init_provide();
            if host
                .write_target(inner, provide.reborrow().init_target())
                .is_some()
            {
                return Ok(None);
            }
            provide
                .get_recipient()
                .set_as(recipient.get_root_as_reader::<any_pointer::Reader>()?)?;
        }
        let mut question = Question::new();
        question.is_awaiting_return = false; // Provide never returns.
        let id = host.questions.borrow_mut().push(question);
        let (sender, _receiver) = oneshot::channel();
        let reference = Rc::new(RefCell::new(QuestionRef::new(host.clone(), id, sender)));
        host.questions.borrow_mut().find(id).unwrap().self_ref = Some(Rc::downgrade(&reference));
        if let message::Provide(provide) =
            message.get_body()?.get_as::<message::Builder>()?.which()?
        {
            provide?.set_question_id(id.to_wire());
        }
        let _ = message.send_detached();
        let mut export = Export::new(inner.add_ref());
        export.vine = Some(Rc::new(move |embargo| {
            let reference = reference.borrow();
            let Some(state) = reference.connection_state.as_ref() else {
                return Ok(());
            };
            let Ok(mut message) = state.new_outgoing_message(32) else {
                // Losing the provision's connection must not abort this
                // recipient's otherwise healthy connection. The lost provision
                // will reject any acceptance still waiting for its embargo.
                return Ok(());
            };
            let mut d = message
                .get_body()?
                .init_as::<message::Builder>()
                .init_disembargo();
            d.reborrow()
                .init_target()
                .init_promised_answer()
                .set_question_id(reference.id.to_wire());
            d.init_context().set_accept(embargo);
            let _ = message.send_detached();
            Ok(())
        }));
        let export_id = state.exports.borrow_mut().push(export);
        let mut third = descriptor.reborrow().init_third_party_hosted();
        third.set_vine_id(export_id.to_wire());
        third
            .get_id()
            .set_as(contact.get_root_as_reader::<any_pointer::Reader>()?)?;
        Ok(Some(export_id))
    }

    pub(super) fn receive_third_party(
        state: &Rc<Self>,
        third: crate::rpc_capnp::third_party_cap_descriptor::Reader<'_>,
        fd: Option<AttachedFd>,
    ) -> capnp::Result<Box<dyn ClientHook>> {
        let vine = Self::import(state, ImportId::from_wire(third.get_vine_id()), false, fd);
        let mut contact = capnp::message::Builder::new_default();
        contact.set_root(third.get_id())?;
        Ok(ThirdPartyClient::deferred(state, Rc::new(contact), vine))
    }

    pub(super) fn accept_third_party(
        state: &Rc<Self>,
        contact: any_pointer::Reader<'_>,
        vine: Box<dyn ClientHook>,
    ) -> capnp::Result<Box<dyn ClientHook>> {
        let mut completion = capnp::message::Builder::new_default();
        let connection = state
            .connection
            .borrow_mut()
            .as_mut()
            .map_err(|e| e.clone())?
            .connect_to_introduced(contact, completion.init_root())?;
        let embargo = state
            .connection
            .borrow_mut()
            .as_mut()
            .map_err(|e| e.clone())?
            .generate_embargo_id()?;
        if embargo.is_empty() {
            return Err(Error::failed("network generated empty embargo ID".into()));
        }
        let mut message = state.new_outgoing_message(32)?;
        let mut d = message
            .get_body()?
            .init_as::<message::Builder>()
            .init_disembargo();
        if state
            .write_target(&*vine, d.reborrow().init_target())
            .is_some()
        {
            return Err(Error::failed("third-party vine changed connection".into()));
        }
        d.init_context().set_accept(&embargo);
        let _ = message.send_detached();
        let Some(connection) = connection else {
            let completion = state
                .connection
                .borrow_mut()
                .as_mut()
                .map_err(|e| e.clone())?
                .complete_third_party_local(completion.get_root_as_reader()?);
            let client: capnp::capability::Client = crate::new_future_client(async move {
                let exchange = completion.await?;
                let accepting = exchange.accept(Some(embargo));
                drop(exchange);
                let result = accepting.await.map(capnp::capability::Client::new);
                drop(vine);
                result
            });
            return Ok(client.hook);
        };
        let registry = state
            .registry
            .upgrade()
            .ok_or_else(|| Error::disconnected("RPC system ended".into()))?;
        if registry.borrow().closing {
            return Err(Error::disconnected("RPC system is closing".into()));
        }
        let host = crate::RpcSystem::get_connection_state(
            &registry,
            state.bootstrap.clone(),
            connection,
            state.system_tasks.clone(),
        );
        let question_id = host.questions.borrow_mut().push(Question::new());
        let (sender, receiver) = oneshot::channel();
        let reference = Rc::new(RefCell::new(QuestionRef::new(
            host.clone(),
            question_id,
            sender,
        )));
        host.questions
            .borrow_mut()
            .find(question_id)
            .unwrap()
            .self_ref = Some(Rc::downgrade(&reference));
        let mut message = host.new_outgoing_message(32)?;
        let mut accept = message
            .get_body()?
            .init_as::<message::Builder>()
            .init_accept();
        accept.set_question_id(question_id.to_wire());
        accept.set_embargo(&embargo);
        accept
            .get_provision()
            .set_as(completion.get_root_as_reader::<any_pointer::Reader>()?)?;
        let _ = message.send_detached();
        let promise = receiver
            .map_err(crate::canceled_to_error)
            .and_then(|p| p)
            .attach((reference.clone(), vine));
        let pipeline = Pipeline::new(&host, reference, Some(Promise::from_future(promise)));
        Ok(pipeline.get_pipelined_cap(&[]))
    }

    pub(super) fn write_descriptor(
        state: &Rc<Self>,
        mut inner: Box<dyn ClientHook>,
        mut descriptor: cap_descriptor::Builder,
        fds: &mut OutgoingFds,
    ) -> ::capnp::Result<Option<ExportId>> {
        // Find the innermost wrapped capability.
        while let Some(resolved) = inner.get_resolved() {
            inner = resolved;
        }
        fds.add(&*inner, descriptor.reborrow());
        if inner.get_brand() == state.get_brand() {
            let Some(c) = Client::from_ptr(inner.get_ptr(), state) else {
                unreachable!()
            };
            Ok(c.write_descriptor(descriptor, fds))
        } else {
            match Self::try_introduce(state, &*inner, descriptor.reborrow()) {
                Ok(Some(export)) => return Ok(Some(export)),
                Ok(None) => (),
                // Transport-specific introduction/forwarding can fail. Export
                // that failure as a broken capability, rather than panicking
                // in payload serialization or breaking unrelated capabilities.
                Err(error) => inner = broken::new_cap(error),
            }
            let ptr = inner.get_ptr();
            let contains_key = state.exports_by_cap.borrow().contains_key(&ptr);
            if contains_key {
                // We've already seen and exported this capability before.  Just up the refcount.
                let export_id = state.exports_by_cap.borrow()[&ptr];
                descriptor.set_sender_hosted(export_id.to_wire());
                // Should never fail because exports_by_cap should match exports.
                state.exports.borrow_mut().find(export_id).unwrap().refcount += 1;
                Ok(Some(export_id))
            } else {
                // This is the first time we've seen this capability.

                let mut exp = Export::new(inner.clone());
                exp.canonical = true;
                let export_id = state.exports.borrow_mut().push(exp);
                state.exports_by_cap.borrow_mut().insert(ptr, export_id);
                match inner.when_more_resolved() {
                    Some(wrapped) => {
                        // This is a promise.  Arrange for the `Resolve` message to be sent later.
                        if let Some(exp) = state.exports.borrow_mut().find(export_id) {
                            exp.resolve_op =
                                Self::resolve_exported_promise(state, export_id, wrapped);
                        }
                        descriptor.set_sender_promise(export_id.to_wire());
                    }
                    None => {
                        descriptor.set_sender_hosted(export_id.to_wire());
                    }
                }
                Ok(Some(export_id))
            }
        }
    }

    pub(super) fn write_descriptors(
        state: &Rc<Self>,
        cap_table: &[Option<Box<dyn ClientHook>>],
        payload: payload::Builder,
        fds: &mut OutgoingFds,
    ) -> Vec<ExportId> {
        // A null table already denotes zero capabilities. Like the C++ RPC
        // implementation, avoid allocating a composite-list tag for every
        // data-only request and reply.
        if cap_table.is_empty() {
            return Vec::new();
        }
        let mut cap_table_builder = payload.init_cap_table(cap_table.len() as u32);
        let mut exports = Vec::new();
        for (idx, value) in cap_table.iter().enumerate() {
            match value {
                Some(cap) => {
                    if let Some(export_id) = Self::write_descriptor(
                        state,
                        cap.clone(),
                        cap_table_builder.reborrow().get(idx as u32),
                        fds,
                    )
                    .unwrap()
                    {
                        exports.push(export_id);
                    }
                }
                None => {
                    cap_table_builder.reborrow().get(idx as u32).set_none(());
                }
            }
        }
        exports
    }

    pub(super) fn import(
        state: &Rc<Self>,
        import_id: ImportId,
        is_promise: bool,
        fd: Option<AttachedFd>,
    ) -> Box<dyn ClientHook> {
        let import_client = {
            let mut imports = state.imports.borrow_mut();
            if let Some(import) = imports.slots.get(&import_id) {
                import
                    .import_client
                    .upgrade()
                    .expect("dangling ref to import client?")
            } else {
                let import_client = ImportClient::new(state, import_id);
                imports.slots.insert(import_id, Import::new(&import_client));
                import_client
            }
        };

        if import_client.borrow().fd.is_none() {
            import_client.borrow_mut().fd = fd;
        }

        // We just received a copy of this import ID, so the remote refcount has gone up.
        import_client.borrow_mut().add_remote_ref();

        let mut tmp = state.imports.borrow_mut();
        let Some(import) = tmp.slots.get_mut(&import_id) else {
            unreachable!()
        };

        if is_promise {
            // We need to construct a PromiseClient around this import, if we haven't already.
            match &import.app_client {
                Some(c) => {
                    // Use the existing one.
                    Box::new(c.upgrade().expect("dangling client ref?"))
                }
                None => {
                    // Create a promise for this import's resolution.

                    let client: Box<Client<VatId>> = Box::new(import_client.into());
                    let client: Box<dyn ClientHook> = client;

                    // Here the C++ implementation does something like:
                    // ```
                    //   // Make sure the import is not destroyed while this promise exists.
                    //   let promise = promise.attach(client.add_ref());
                    // ```
                    // However, as far as I can tell that is unnecessary, because the
                    // PromiseClient holds `client` until it resolves, after which point
                    // there is no reason to keep the import alive.

                    let client = PromiseClient::new(state, client, Some(import_id));

                    import.promise_client_to_resolve = Some(Rc::downgrade(&client));
                    let client: Box<Client<VatId>> = Box::new(client.into());
                    import.app_client = Some(client.downgrade());
                    client
                }
            }
        } else {
            let client: Box<Client<VatId>> = Box::new(import_client.into());
            import.app_client = Some(client.downgrade());
            client
        }
    }

    pub(super) fn receive_cap(
        state: &Rc<Self>,
        descriptor: cap_descriptor::Reader,
        fds: &mut IncomingFds,
    ) -> ::capnp::Result<Option<Box<dyn ClientHook>>> {
        let fd = fds.take(descriptor.get_attached_fd());
        match descriptor.which()? {
            cap_descriptor::None(()) => Ok(None),
            cap_descriptor::SenderHosted(sender_hosted) => Ok(Some(Self::import(
                state,
                ImportId::from_wire(sender_hosted),
                false,
                fd,
            ))),
            cap_descriptor::SenderPromise(sender_promise) => Ok(Some(Self::import(
                state,
                ImportId::from_wire(sender_promise),
                true,
                fd,
            ))),
            cap_descriptor::ReceiverHosted(receiver_hosted) => {
                if let Some(exp) = state
                    .exports
                    .borrow_mut()
                    .find(ExportId::from_wire(receiver_hosted))
                {
                    Ok(Some(match &exp.reflected_vine {
                        Some(recreate) => recreate(),
                        None => exp.client_hook.add_ref(),
                    }))
                } else {
                    Ok(Some(broken::new_cap(Error::failed(
                        "invalid 'receiverHosted' export ID".to_string(),
                    ))))
                }
            }
            cap_descriptor::ReceiverAnswer(receiver_answer) => {
                let promised_answer = receiver_answer?;
                let question_id = AnswerId::from_wire(promised_answer.get_question_id());
                if let Some(answer) = state.answers.borrow().slots.get(&question_id) {
                    if let Some(ref pipeline) = answer.pipeline {
                        let ops = to_pipeline_ops(promised_answer.get_transform()?)?;
                        return Ok(Some(pipeline.get_pipelined_cap(&ops)));
                    }
                }
                Ok(Some(broken::new_cap(Error::failed(
                    "invalid 'receiver answer'".to_string(),
                ))))
            }
            cap_descriptor::ThirdPartyHosted(third) => {
                Ok(Some(Self::receive_third_party(state, third?, fd)?))
            }
        }
    }

    pub(super) fn receive_caps(
        state: &Rc<Self>,
        cap_table: ::capnp::struct_list::Reader<cap_descriptor::Owned>,
        fds: &mut IncomingFds,
    ) -> ::capnp::Result<Vec<Option<Box<dyn ClientHook>>>> {
        let mut result = Vec::new();
        for idx in 0..cap_table.len() {
            result.push(Self::receive_cap(state, cap_table.get(idx), fds)?);
        }
        Ok(result)
    }
}
