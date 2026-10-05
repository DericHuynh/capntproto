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

//! Incoming RPC dispatch and peer message validation.
use super::*;

impl<VatId: 'static> ConnectionState<VatId> {
    pub(super) fn send_unimplemented(
        connection_state: &Rc<Self>,
        message: &dyn crate::IncomingMessage,
    ) -> capnp::Result<()> {
        // An unimplemented reply copies an arbitrary peer message. Let the
        // transport choose its default segment size rather than guessing 50 words.
        let mut out_message = connection_state.new_outgoing_message(0)?;
        {
            let mut root: message::Builder = out_message.get_body()?.get_as()?;
            root.set_unimplemented(message.get_body()?.get_as()?)?;
        }
        let _ = out_message.send_detached();
        Ok(())
    }

    pub(super) fn handle_unimplemented(
        connection_state: &Rc<Self>,
        message: message::Reader,
    ) -> capnp::Result<()> {
        match message.which()? {
            message::Join(request) => {
                join::unimplemented(connection_state, request?)?;
            }
            message::Resolve(resolve) => {
                let resolve = resolve?;
                match resolve.which()? {
                    resolve::Cap(c) => match c?.which()? {
                        cap_descriptor::None(()) => (),
                        cap_descriptor::SenderHosted(export_id) => {
                            connection_state.release_export(ExportId::from_wire(export_id), 1)?;
                        }
                        cap_descriptor::SenderPromise(export_id) => {
                            connection_state.release_export(ExportId::from_wire(export_id), 1)?;
                        }
                        cap_descriptor::ReceiverAnswer(_) | cap_descriptor::ReceiverHosted(_) => (),
                        cap_descriptor::ThirdPartyHosted(_) => {
                            return Err(Error::failed(
                                "Peer claims we resolved a ThirdPartyHosted cap.".to_string(),
                            ));
                        }
                    },
                    resolve::Exception(_) => (),
                }
            }
            _ => {
                return Err(Error::failed(
                    "Peer did not implement required RPC message type.".to_string(),
                ));
            }
        }
        Ok(())
    }

    pub(super) fn handle_bootstrap(
        connection_state: &Rc<Self>,
        bootstrap: bootstrap::Reader,
    ) -> capnp::Result<()> {
        use ::capnp::traits::ImbueMut;

        let answer_id = AnswerId::from_wire(bootstrap.get_question_id());
        answer_adoption::ordinary_id(answer_id)?;
        if connection_state.connection.borrow().is_err() {
            // Disconnected; ignore.
            return Ok(());
        }

        if connection_state
            .answers
            .borrow()
            .slots
            .contains_key(&answer_id)
        {
            return Err(Error::failed("questionId is already in use".into()));
        }
        // Release the transport borrow before entering application factory code.
        let peer = connection_state
            .connection
            .borrow()
            .as_ref()
            .unwrap()
            .get_peer_vat_id();
        let capability = if bootstrap.has_deprecated_object_id() {
            Err(Error::failed(
                "named exports are obsolete; use the bootstrap interface".into(),
            ))
        } else {
            connection_state.bootstrap.for_peer(&peer)
        };
        let mut fds = OutgoingFds::default();
        let mut response = connection_state.new_outgoing_message(10)?;
        let mut ret = response
            .get_body()?
            .init_as::<message::Builder>()
            .init_return();
        ret.set_answer_id(answer_id.to_wire());
        let (cap, result_exports) = match capability {
            Ok(cap) => {
                let mut cap_table = Vec::new();
                let mut payload = ret.init_results();
                {
                    let mut content = payload.reborrow().get_content();
                    content.imbue_mut(&mut cap_table);
                    content.set_as_capability(cap.clone());
                }
                let exports =
                    Self::write_descriptors(connection_state, &cap_table, payload, &mut fds);
                (cap, exports)
            }
            Err(error) => {
                from_error(
                    &error,
                    ret.init_exception(),
                    connection_state.encode_trace(&error).as_deref(),
                );
                (broken::new_cap(error), Vec::new())
            }
        };
        let mut answer = Answer::new();
        answer.status.return_has_been_sent.set(true);
        answer.result_exports = result_exports;
        answer.pipeline = Some(Box::new(SingleCapPipeline::new(cap)));
        connection_state
            .answers
            .borrow_mut()
            .slots
            .insert(answer_id, answer);
        fds.attach(&mut *response);
        let _ = response.send_detached();
        Ok(())
    }

    pub(super) fn handle_finish(
        connection_state: &Rc<Self>,
        finish: finish::Reader,
    ) -> capnp::Result<()> {
        let mut exports = Vec::new();
        let mut pipeline = None;
        let mut task = None;
        let mut answer_to_release = None;
        let mut pipeline_only_guard = None;
        let mut join = None;
        {
            let mut answers = connection_state.answers.borrow_mut();
            if let hash_map::Entry::Occupied(mut entry) = answers
                .slots
                .entry(AnswerId::from_wire(finish.get_question_id()))
            {
                let answer = entry.get_mut();
                answer.status.received_finish.set(true);
                let result_exports = mem::take(&mut answer.result_exports);
                if finish.get_release_result_caps() {
                    exports = result_exports;
                }
                pipeline = answer.pipeline.take();
                task = answer.call_completion_promise.take();
                pipeline_only_guard = answer.pipeline_only_guard.take();
                join = answer.join.take();
                if answer.status.return_has_been_sent.get() {
                    answer_to_release = Some(entry.remove());
                }
            }
        }
        // A call context's destructor may send its Return and erase the answer.
        // Do not run destructors while borrowing the table they can re-enter.
        drop(pipeline);
        drop(join);
        if finish.get_require_early_cancellation_workaround() {
            connection_state.add_task(async move {
                yield_once().await;
                drop(task);
                Ok(())
            });
        } else {
            drop(task);
        }
        drop(answer_to_release);
        drop(pipeline_only_guard);
        connection_state.release_exports(&exports)
    }

    pub(super) fn handle_resolve(
        connection_state: &Rc<Self>,
        resolve: resolve::Reader,
        fds: &mut IncomingFds,
    ) -> capnp::Result<()> {
        let replacement_or_error = match resolve.which()? {
            resolve::Cap(c) => match Self::receive_cap(connection_state, c?, fds)? {
                Some(cap) => Ok(cap),
                None => {
                    return Err(Error::failed(
                        "'Resolve' contained 'CapDescriptor.none'.".to_string(),
                    ));
                }
            },
            resolve::Exception(e) => {
                // We can't set `replacement` to a new broken cap here because this will
                // confuse PromiseClient::Resolve() into thinking that the remote
                // promise resolved to a local capability and therefore a Disembargo is
                // needed. We must actually reject the promise.
                Err(remote_exception_to_error(e?))
            }
        };

        // If the import is in the table, fulfill it.
        let slots = &mut connection_state.imports.borrow_mut().slots;
        if let Some(import) = slots.get_mut(&ImportId::from_wire(resolve.get_promise_id())) {
            match import.promise_client_to_resolve.take() {
                Some(weak_promise_client) => {
                    if let Some(promise_client) = weak_promise_client.upgrade() {
                        promise_client.borrow_mut().resolve(replacement_or_error);
                    }
                }
                None => {
                    return Err(Error::failed(
                        "Got 'Resolve' for a non-promise import.".to_string(),
                    ));
                }
            }
        }
        Ok(())
    }

    pub(super) fn handle_disembargo(
        connection_state: &Rc<Self>,
        disembargo: disembargo::Reader,
    ) -> capnp::Result<()> {
        let context = disembargo.get_context();
        match context.which()? {
            disembargo::context::SenderLoopback(embargo_id) => {
                // Opaque peer-owned context: echo it without interpreting it as
                // an entry in our local embargo table.
                let mut target = connection_state.get_message_target(disembargo.get_target()?)?;
                let connection_state_ref = connection_state.clone();
                let connection_state_ref1 = connection_state.clone();
                let task = async move {
                    target.when_resolved().await?;
                    // Let queued calls made before resolution forward first.
                    let mut yielded = false;
                    future::poll_fn(move |cx| {
                        if yielded {
                            Poll::Ready(())
                        } else {
                            yielded = true;
                            cx.waker().wake_by_ref();
                            Poll::Pending
                        }
                    })
                    .await;
                    while let Some(resolved) = target.get_resolved() {
                        target = resolved;
                    }
                    if target.get_brand() != connection_state_ref.get_brand() {
                        return Err(Error::failed(
                            "senderLoopback target does not point back to sender".into(),
                        ));
                    }
                    if let Ok(ref mut c) = *connection_state_ref.connection.borrow_mut() {
                        let mut message = c.new_outgoing_message(100); // TODO estimate size
                        {
                            let root: message::Builder = message.get_body()?.init_as();
                            let mut disembargo = root.init_disembargo();
                            disembargo
                                .reborrow()
                                .init_context()
                                .set_receiver_loopback(embargo_id);

                            let redirect =
                                match Client::from_ptr(target.get_ptr(), &connection_state_ref1) {
                                    Some(c) => c.write_target(disembargo.init_target()),
                                    None => unreachable!(),
                                };
                            if redirect.is_some() {
                                return Err(Error::failed(
                                    "'Disembargo' of type 'senderLoopback' sent to an object that \
                                     does not appear to have been the subject of a previous \
                                     'Resolve' message."
                                        .to_string(),
                                ));
                            }
                        }
                        let _ = message.send_detached();
                    }
                    Ok(())
                };
                connection_state.add_task(task);
            }
            disembargo::context::ReceiverLoopback(embargo_id) => {
                let embargo_id = EmbargoId::from_wire(embargo_id);
                if let Some(embargo) = connection_state.embargoes.borrow_mut().find(embargo_id) {
                    let fulfiller = embargo.fulfiller.take().unwrap();
                    let _ = fulfiller.send(Ok(()));
                } else {
                    return Err(Error::failed(
                        "Invalid embargo ID in `Disembargo.context.receiverLoopback".to_string(),
                    ));
                }
                connection_state.embargoes.borrow_mut().erase(embargo_id);
            }
            disembargo::context::Accept(id) => {
                let target = disembargo.get_target()?;
                match target.which()? {
                    message_target::PromisedAnswer(answer) => {
                        let answer = answer?;
                        if !answer.get_transform()?.is_empty() {
                            return Err(Error::failed(
                                "Provide disembargo cannot have a transform".into(),
                            ));
                        }
                        let exchange = connection_state
                            .answers
                            .borrow()
                            .slots
                            .get(&AnswerId::from_wire(answer.get_question_id()))
                            .and_then(|answer| answer.provision.as_ref().map(|p| p.0.clone()))
                            .ok_or_else(|| {
                                Error::failed("disembargo does not identify a Provide".into())
                            })?;
                        exchange.disembargo(id?)?;
                    }
                    message_target::ImportedCap(export) => {
                        let forward = connection_state
                            .exports
                            .borrow_mut()
                            .find(ExportId::from_wire(export))
                            .and_then(|e| e.vine.clone())
                            .ok_or_else(|| {
                                Error::failed("disembargo target is not a vine".into())
                            })?;
                        forward(id?)?;
                    }
                }
            }
        }
        Ok(())
    }

    pub(super) fn handle_provide(
        state: &Rc<Self>,
        provide: crate::rpc_capnp::provide::Reader<'_>,
    ) -> capnp::Result<()> {
        let id = AnswerId::from_wire(provide.get_question_id());
        answer_adoption::ordinary_id(id)?;
        if state.answers.borrow().slots.contains_key(&id) {
            return Err(Error::failed("questionId is already in use".into()));
        }
        let target = state.get_message_target(provide.get_target()?)?;
        let exchange = crate::third_party::ThirdPartyExchange::new(target);
        let registration = state
            .connection
            .borrow_mut()
            .as_mut()
            .map_err(|e| e.clone())?
            .await_third_party(provide.get_recipient(), exchange.clone())?;
        let mut answer = Answer::new();
        // Provide intentionally never sends Return. Finish can erase it immediately.
        answer.status.return_has_been_sent.set(true);
        answer.provision = Some((exchange, registration));
        answer.pipeline = Some(Box::new(broken::Pipeline::new(Error::failed(
            "cannot pipeline on Provide".into(),
        ))));
        state.answers.borrow_mut().slots.insert(id, answer);
        Ok(())
    }

    pub(super) fn handle_accept(
        state: &Rc<Self>,
        accept: crate::rpc_capnp::accept::Reader<'_>,
    ) -> capnp::Result<()> {
        let id = AnswerId::from_wire(accept.get_question_id());
        answer_adoption::ordinary_id(id)?;
        if state.answers.borrow().slots.contains_key(&id) {
            return Err(Error::failed("questionId is already in use".into()));
        }
        let embargo = if accept.has_embargo() {
            Some(accept.get_embargo()?.to_vec())
        } else {
            None
        };
        let completion = state
            .connection
            .borrow_mut()
            .as_mut()
            .map_err(|e| e.clone())?
            .complete_third_party(accept.get_provision());
        let answer = Answer::new();
        let (sender, receiver) = oneshot::channel();
        let (pipeline_sender, mut pipeline) = queued::Pipeline::new();
        let mut results = Results::new(
            state,
            id,
            false,
            sender,
            answer.status.clone(),
            Some(pipeline_sender.weak_clone()),
        );
        state.answers.borrow_mut().slots.insert(id, answer);
        let task = async move {
            let status = async {
                let exchange = completion.await?;
                let accepting = exchange.accept(embargo);
                drop(exchange);
                let cap = accepting.await?;
                results.get()?.set_as_capability(cap);
                Ok(())
            }
            .await;
            drop(results);
            // Reuse the normal Return and pipelining machinery, including Finish,
            // result export reference counts, and exception delivery.
            let _ = ResultsDone::from_results_inner(
                receiver.await.map_err(crate::canceled_to_error),
                status,
                pipeline_sender,
            );
            Ok(())
        }
        .boxed_local()
        .shared();
        pipeline.drive(task.clone());
        let running = state.eagerly_evaluate(task);
        let mut answers = state.answers.borrow_mut();
        let answer = answers.slots.get_mut(&id).unwrap();
        answer.pipeline = Some(Box::new(pipeline));
        answer.call_completion_promise = Some(running);
        Ok(())
    }

    pub(super) fn handle_message(
        weak_state: &Weak<Self>,
        mut message: Box<dyn crate::IncomingMessage>,
    ) -> ::capnp::Result<()> {
        let Some(connection_state) = weak_state.upgrade() else {
            return Err(Error::disconnected(
                "handle_message() cannot continue without a connection".into(),
            ));
        };

        if connection_state.connection.borrow().is_err() {
            return Ok(());
        }
        connection_state.set_not_idle();
        let mut fds = IncomingFds::new(&mut *message);
        let reader = message.get_body()?.get_as::<message::Reader>()?;
        match reader.which() {
            Ok(message::Unimplemented(message)) => {
                Self::handle_unimplemented(&connection_state, message?)?
            }
            Ok(message::Abort(abort)) => return Err(remote_exception_to_error(abort?)),
            Ok(message::Bootstrap(bootstrap)) => {
                Self::handle_bootstrap(&connection_state, bootstrap?)?
            }
            Ok(message::Call(call)) => {
                let call = call?;
                answer_adoption::ordinary_id(AnswerId::from_wire(call.get_question_id()))?;
                if let call::send_results_to::ThirdParty(_) = call.get_send_results_to().which()? {
                    return answer_adoption::receive_call(&connection_state, message, &mut fds);
                }
                let capability = connection_state.get_message_target(call.get_target()?)?;
                let (interface_id, method_id, question_id, cap_table_array, redirect_results) = {
                    let redirect_results = match call.get_send_results_to().which()? {
                        call::send_results_to::Caller(()) => false,
                        call::send_results_to::Yourself(()) => true,
                        call::send_results_to::ThirdParty(_) => {
                            return Err(Error::failed(
                                "Unsupported `Call.sendResultsTo`.".to_string(),
                            ))
                        }
                    };
                    let payload = call.get_params()?;

                    (
                        call.get_interface_id(),
                        call.get_method_id(),
                        AnswerId::from_wire(call.get_question_id()),
                        Self::receive_caps(&connection_state, payload.get_cap_table()?, &mut fds)?,
                        redirect_results,
                    )
                };

                if connection_state
                    .answers
                    .borrow()
                    .slots
                    .contains_key(&question_id)
                {
                    return Err(Error::failed(format!(
                        "Received a new call on in-use question id {question_id}"
                    )));
                }

                let request_words = message.size_in_words();
                let total = connection_state
                    .call_words
                    .get()
                    .checked_add(request_words)
                    .ok_or_else(|| Error::overloaded("incoming call word count overflow".into()))?;
                let only_promise_pipeline = call.get_only_promise_pipeline() && !redirect_results;
                let hints = CallHints {
                    only_promise_pipeline,
                    no_promise_pipelining: call.get_no_promise_pipelining(),
                };
                let allow_third_party = call.get_allow_third_party_tail_call();
                let params = Params::new(message, cap_table_array);
                let mut answer = Answer::new();
                answer.request_words = request_words;
                connection_state.call_words.set(total);

                let (results_inner_fulfiller, results_inner_promise) = oneshot::channel();
                let results_inner_promise = results_inner_promise.map_err(crate::canceled_to_error);

                let (pipeline_sender, pipeline) =
                    if hints.no_promise_pipelining && !only_promise_pipeline && !redirect_results {
                        (queued::PipelineInnerSender::disabled(), None)
                    } else {
                        let (sender, pipeline) = queued::Pipeline::new();
                        (sender, Some(pipeline))
                    };
                let mut results = Results::new(
                    &connection_state,
                    question_id,
                    redirect_results,
                    results_inner_fulfiller,
                    answer.status.clone(),
                    Some(pipeline_sender.weak_clone()),
                );

                let inner = results.inner.as_mut().unwrap();
                inner.allow_third_party = allow_third_party;
                inner.only_promise_pipeline = only_promise_pipeline;
                if only_promise_pipeline {
                    Rc::get_mut(&mut inner.return_guard).unwrap().only_pipeline = true;
                    answer.pipeline_only_guard = Some(inner.return_guard.clone());
                }

                let (redirected_results_done_promise, redirected_results_done_fulfiller) =
                    if redirect_results {
                        let (f, p) = oneshot::channel::<Result<Response<VatId>, Error>>();
                        let p = p.map_err(crate::canceled_to_error).and_then(future::ready);
                        (Some(Promise::from_future(p)), Some(f))
                    } else {
                        (None, None)
                    };

                {
                    let slots = &mut connection_state.answers.borrow_mut().slots;
                    let hash_map::Entry::Vacant(slot) = slots.entry(question_id) else {
                        return Err(Error::failed("questionId is already in use".to_string()));
                    };
                    slot.insert(answer);
                }

                // No table borrow spans application code. Immediate ordinary
                // calls need no independently scheduled cancellation owner.
                results.permits_immediate_poll = pipeline.is_none();
                let call_promise = capability.call_with_hints(
                    interface_id,
                    method_id,
                    Box::new(params),
                    Box::new(results),
                    hints,
                );

                let mut promise = call_promise
                    .then(move |call_result| {
                        results_inner_promise.then(move |result| {
                            future::ready(ResultsDone::from_results_inner(
                                result,
                                call_result,
                                pipeline_sender,
                            ))
                        })
                    })
                    .then(move |v| {
                        if let Some(f) = redirected_results_done_fulfiller {
                            match v.and_then(ResultsCompletion::into_retained) {
                                Ok(r) => drop(f.send(Ok(Response::redirected(r)))),
                                Err(e) => drop(f.send(Err(e))),
                            }
                        }
                        Promise::ok(())
                    });

                // A non-pipelined call may complete on its first poll. Drive
                // that bounded step before allocating a background task and
                // two cancellation/completion channels. No table borrow may
                // span application code. Pending work is always enqueued and
                // polled again with the task set's real waker.
                let completed = pipeline.is_none() && (&mut promise).now_or_never().is_some();
                if !completed {
                    let slots = &mut connection_state.answers.borrow_mut().slots;
                    let Some(answer) = slots.get_mut(&question_id) else {
                        // The first poll may synchronously disconnect the vat.
                        return Ok(());
                    };
                    if redirect_results {
                        answer.redirected_results = redirected_results_done_promise;
                    }
                    answer.call_completion_promise = Some(if let Some(mut pipeline) = pipeline {
                        let fork = promise.shared();
                        pipeline.drive(fork.clone());
                        answer.pipeline = Some(Box::new(pipeline));
                        connection_state.eagerly_evaluate(fork)
                    } else {
                        connection_state.eagerly_evaluate(promise)
                    });
                }
            }
            Ok(message::Return(oret)) => {
                let ret = oret?;
                let question_id = QuestionId::from_wire(ret.get_answer_id());
                if question_id.is_pipeline_only() {
                    // A legacy peer ignored onlyPromisePipeline. Never interpret
                    // this Return using a possibly already-retired high ID.
                    connection_state.got_return_for_high_id.set(true);
                    return Ok(());
                }
                let (reference, is_tail, param_exports) = {
                    let mut questions = connection_state.questions.borrow_mut();
                    let question = questions.find(question_id).ok_or_else(|| {
                        Error::failed(format!("Invalid question ID in Return: {question_id}"))
                    })?;
                    if !question.is_awaiting_return {
                        return Err(Error::failed(
                            "duplicate Return or Return for Provide".into(),
                        ));
                    }
                    if (question.join_id.is_some()
                        || question.multiparty_join
                        || question_id.is_adopted())
                        && ret.get_no_finish_needed()
                    {
                        return Err(Error::failed(
                            "Join and adopted results require Finish".into(),
                        ));
                    }
                    if matches!(ret.which()?, return_::AwaitFromThirdParty(_))
                        && (!question.allow_third_party || ret.get_no_finish_needed())
                    {
                        return Err(Error::failed("unauthorized third-party tail return".into()));
                    }
                    question.is_awaiting_return = false;
                    question.skip_finish = ret.get_no_finish_needed();
                    (
                        question.self_ref.as_ref().and_then(Weak::upgrade),
                        question.is_tail_call,
                        mem::take(&mut question.param_exports),
                    )
                };
                // Releasing exports or receiving third-party descriptors can re-enter
                // question allocation. Never hold the question table borrow here.
                if ret.get_release_param_caps() {
                    connection_state.release_exports(&param_exports)?;
                }
                if let Some(reference) = reference {
                    match ret.which()? {
                        return_::Results(results) => {
                            if is_tail {
                                return Err(Error::failed(
                                    "tail call returned ordinary results".into(),
                                ));
                            }
                            let caps = Self::receive_caps(
                                &connection_state,
                                results?.get_cap_table()?,
                                &mut fds,
                            )?;
                            let response = Response::new(
                                connection_state.clone(),
                                reference.clone(),
                                message,
                                caps,
                            );
                            reference.borrow_mut().fulfill(Promise::ok(response));
                        }
                        return_::Exception(exception) => {
                            if is_tail {
                                return Err(Error::failed(
                                    "tail call returned ordinary exception".into(),
                                ));
                            }
                            reference
                                .borrow_mut()
                                .reject(remote_exception_to_error(exception?));
                        }
                        return_::Canceled(()) => {
                            return Err(Error::failed(
                                "peer canceled an outstanding question".into(),
                            ))
                        }
                        return_::ResultsSentElsewhere(()) => {
                            if !is_tail {
                                return Err(Error::failed(
                                    "resultsSentElsewhere for non-tail call".into(),
                                ));
                            }
                            let empty = local::ResultsDone::new(
                                capnp::message::Builder::new_default(),
                                vec![],
                            );
                            reference
                                .borrow_mut()
                                .fulfill(Promise::ok(Response::redirected(Box::new(empty))));
                        }
                        return_::TakeFromOtherQuestion(id) => {
                            let id = AnswerId::from_wire(id);
                            if is_tail {
                                return Err(Error::failed(
                                    "tail call cannot take another answer".into(),
                                ));
                            }
                            let (response, responded, task) = {
                                let mut answers = connection_state.answers.borrow_mut();
                                let answer = answers.slots.get_mut(&id).ok_or_else(|| {
                                    Error::failed("takeFromOtherQuestion: invalid answer".into())
                                })?;
                                let response = answer.redirected_results.take().ok_or_else(|| Error::failed("takeFromOtherQuestion: already adopted or not redirected".into()))?;
                                (
                                    response,
                                    answer.status.clone(),
                                    answer.call_completion_promise.take(),
                                )
                            };
                            // Taking the response also takes ownership of its producer.
                            // Finish may release the old answer before the adopted call
                            // completes; the adopting question must keep it running.
                            reference
                                .borrow_mut()
                                .fulfill(Promise::from_future(response.attach(task)));
                            connection_state.acknowledge_redirected_answer(
                                id,
                                &responded.return_has_been_sent,
                            )?;
                        }
                        return_::AwaitFromThirdParty(token) => {
                            let pipeline = reference.borrow().pipeline.clone();
                            let response =
                                answer_adoption::await_answer(&connection_state, token, pipeline);
                            reference.borrow_mut().fulfill(response);
                        }
                    }
                } else {
                    // A late tail-call response still transfers ownership, even if
                    // the adopting caller has gone away. Release its producer and
                    // acknowledge the redirect so the peer can Finish the old answer.
                    if let return_::TakeFromOtherQuestion(id) = ret.which()? {
                        let id = AnswerId::from_wire(id);
                        let redirected = {
                            let mut answers = connection_state.answers.borrow_mut();
                            answers.slots.get_mut(&id).map(|answer| {
                                (
                                    answer.redirected_results.take(),
                                    answer.call_completion_promise.take(),
                                    answer.status.clone(),
                                )
                            })
                        };
                        if let Some((response, task, responded)) = redirected {
                            connection_state.acknowledge_redirected_answer(
                                id,
                                &responded.return_has_been_sent,
                            )?;
                            drop(response);
                            drop(task);
                        }
                    }
                    if let return_::AwaitFromThirdParty(token) = ret.which()? {
                        // Register and immediately retire the rendezvous so an early
                        // or late adoption releases its callee-allocated question.
                        drop(answer_adoption::await_answer(
                            &connection_state,
                            token,
                            None,
                        ));
                    }
                    // Finish was already sent with releaseResultCaps=true.
                    connection_state.questions.borrow_mut().erase(question_id);
                }
            }
            Ok(message::Finish(finish)) => Self::handle_finish(&connection_state, finish?)?,
            Ok(message::Resolve(resolve)) => {
                Self::handle_resolve(&connection_state, resolve?, &mut fds)?
            }
            Ok(message::Release(release)) => {
                let release = release?;
                connection_state.release_export(
                    ExportId::from_wire(release.get_id()),
                    release.get_reference_count(),
                )?;
            }
            Ok(message::Disembargo(disembargo)) => {
                Self::handle_disembargo(&connection_state, disembargo?)?
            }
            Ok(message::Provide(provide)) => {
                if connection_state
                    .connection
                    .borrow()
                    .as_ref()
                    .map(|c| c.supports_third_party())
                    .unwrap_or(false)
                {
                    Self::handle_provide(&connection_state, provide?)?;
                } else {
                    Self::send_unimplemented(&connection_state, message.as_ref())?;
                }
            }
            Ok(message::Accept(accept)) => {
                if connection_state
                    .connection
                    .borrow()
                    .as_ref()
                    .map(|c| c.supports_third_party())
                    .unwrap_or(false)
                {
                    Self::handle_accept(&connection_state, accept?)?;
                } else {
                    Self::send_unimplemented(&connection_state, message.as_ref())?;
                }
            }
            Ok(message::Join(request)) => {
                if multiparty_join::supported(&connection_state) {
                    multiparty_join::receive(&connection_state, request?)?;
                } else if join::supported(&connection_state) {
                    join::receive(&connection_state, request?)?;
                } else {
                    Self::send_unimplemented(&connection_state, message.as_ref())?;
                }
            }
            Ok(message::ThirdPartyAnswer(answer)) => {
                if answer_adoption::supported(&connection_state) {
                    answer_adoption::receive(&connection_state, answer?)?;
                } else {
                    Self::send_unimplemented(&connection_state, message.as_ref())?;
                }
            }
            Ok(message::ObsoleteSave(_) | message::ObsoleteDelete(_))
            | Err(::capnp::NotInSchema(_)) => {
                Self::send_unimplemented(&connection_state, message.as_ref())?;
            }
        }
        connection_state.schedule_idle_check();
        Ok(())
    }
}
