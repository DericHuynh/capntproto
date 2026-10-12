//! Generated RPC conversations using the four bounded tables and caller IO.
use super::*;
use capntproto_rpc_engine::{
    answers, exports,
    ids::{AnswerId, ImportId, WireId},
    imports, messages, questions,
};
use capntproto_rpc_wire::{
    admission, calls, inbound, pipeline,
    rpc_capnp::{
        self,
        api::{CapDescriptor, Message, Payload, Return},
    },
};
use portable_capnp::{
    api::echo::{EchoParams, EchoResults},
    echo,
};

pub(super) fn seal<'id>(
    messages: &mut messages::Messages<'id, '_, '_>,
    builder: &message::FixedBuilder<'_>,
) -> messages::Lease<'id> {
    let length = (builder.size_in_words() + 1) * 8;
    let writer = messages.reserve(length).unwrap();
    capnp::serialize::write_message_segments(
        &mut &mut messages.write(&writer)[..length],
        &builder.get_segments_for_output(),
    )
    .unwrap();
    messages.seal(writer, length).unwrap()
}
pub(super) fn receive<'a, 'pool>(
    queue: &mut messages::Queue<'pool, '_>,
    messages: &mut messages::Messages<'pool, '_, '_>,
    input: &'a mut [Word],
    chunk: usize,
) -> capnp::serialize::FrameReceiver<'a> {
    use core::{
        num::NonZeroUsize,
        task::{Context, Poll, Waker},
    };
    let mut wire = [0; 1024];
    let mut io = Pipe {
        bytes: &mut wire,
        read: 0,
        written: 0,
        chunk,
        attempts: 0,
    };
    let mut cx = Context::from_waker(Waker::noop());
    let budget = NonZeroUsize::new(2).unwrap();
    while capntproto_rpc_engine::io::poll_queue(&mut io, queue, messages, &mut cx, budget)
        .map(|result| result.unwrap())
        .is_pending()
    {}
    let mut receiver =
        capnp::serialize::FrameReceiver::new(Word::words_to_bytes_mut(input), Default::default())
            .unwrap();
    loop {
        match capntproto_rpc_wire::io::poll_receive(&mut io, &mut receiver, &mut cx, budget) {
            Poll::Ready(result) => {
                assert!(result.unwrap());
                break;
            }
            Poll::Pending => (),
        }
    }
    assert!(receiver.message().unwrap().is_some());
    receiver
}

pub(super) struct Pipe<'a> {
    bytes: &'a mut [u8],
    read: usize,
    written: usize,
    chunk: usize,
    attempts: usize,
}
impl Pipe<'_> {
    fn attempt(
        &mut self,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<core::result::Result<(), capntproto_rpc_engine::io::IoError<()>>> {
        self.attempts += 1;
        if self.attempts.is_multiple_of(3) {
            cx.waker().wake_by_ref();
            core::task::Poll::Pending
        } else if self.attempts.is_multiple_of(5) {
            core::task::Poll::Ready(Err(capntproto_rpc_engine::io::IoError::Interrupted))
        } else {
            core::task::Poll::Ready(Ok(()))
        }
    }
}
impl<'a> Pipe<'a> {
    pub(super) fn new(bytes: &'a mut [u8], chunk: usize) -> Self {
        Self {
            bytes,
            read: 0,
            written: 0,
            chunk,
            attempts: 0,
        }
    }
}
impl capntproto_rpc_engine::io::Flush for Pipe<'_> {
    type Error = ();
    fn poll_flush(
        &mut self,
        _: &mut core::task::Context<'_>,
    ) -> core::task::Poll<core::result::Result<(), capntproto_rpc_engine::io::IoError<Self::Error>>>
    {
        core::task::Poll::Ready(Ok(()))
    }
}
impl capntproto_rpc_engine::io::Write for Pipe<'_> {
    fn poll_write(
        &mut self,
        cx: &mut core::task::Context<'_>,
        bytes: &[u8],
    ) -> core::task::Poll<core::result::Result<usize, capntproto_rpc_engine::io::IoError<()>>> {
        core::task::ready!(self.attempt(cx))?;
        let n = bytes
            .len()
            .min(self.chunk)
            .min(self.bytes.len() - self.written);
        self.bytes[self.written..self.written + n].copy_from_slice(&bytes[..n]);
        self.written += n;
        core::task::Poll::Ready(Ok(n))
    }
}
impl capntproto_rpc_engine::io::Read for Pipe<'_> {
    type Error = ();
    fn poll_read(
        &mut self,
        cx: &mut core::task::Context<'_>,
        bytes: &mut [u8],
    ) -> core::task::Poll<core::result::Result<usize, capntproto_rpc_engine::io::IoError<()>>> {
        core::task::ready!(self.attempt(cx))?;
        let n = bytes.len().min(self.chunk).min(self.written - self.read);
        bytes[..n].copy_from_slice(&self.bytes[self.read..self.read + n]);
        self.read += n;
        core::task::Poll::Ready(Ok(n))
    }
}

/// Bootstrap -> capability -> generated call -> typed return -> Finish/Release.
/// Also cancel a call before dispatch, including FIFO backpressure and late Return.
pub fn generated_rpc_conversation(chunk: usize, cancel: bool) -> Result<()> {
    let mut question_slots = [const { questions::Slot::EMPTY }; 2];
    let mut export_slots = [exports::Slot::EMPTY];
    let mut import_slots = [imports::Slot::EMPTY];
    let mut words = [ZERO; 96];
    let mut incoming = [ZERO; 97];
    let mut outgoing = [ZERO; 97];
    let mut retained = [ZERO; 97];
    let mut buffers = [
        messages::Buffer::new(Word::words_to_bytes_mut(&mut outgoing)),
        messages::Buffer::new(Word::words_to_bytes_mut(&mut retained)),
    ];
    let mut send_slots = [messages::SendSlot::EMPTY];
    questions::with_questions(&mut question_slots, |questions| {
        exports::with_exports(&mut export_slots, |exports| {
            imports::with_imports(&mut import_slots, |imports| {
                let mut answer_slots = [const { answers::Slot::EMPTY }; 2];
                answers::with_answers(&mut answer_slots, |answers| {
                    messages::with_messages(
                        &mut buffers,
                        &mut send_slots,
                        |messages| -> Result<()> {
                            let mut queue_slots = [None];
                            let mut queue = messages::Queue::new(&mut queue_slots).unwrap();
                            let mut output = message::FixedBuilder::new(&mut words)?;

                            let reservation = questions.reserve().unwrap();
                            calls::bootstrap(&mut output, reservation.id())?;
                            let frame = seal(messages, &output);
                            let original_bootstrap = messages.retain(&frame).unwrap();
                            let bootstrap = admission::question(
                                reservation,
                                messages,
                                frame,
                                Default::default(),
                            )
                            .unwrap()
                            .accept(queue.reserve().unwrap());
                            assert!(matches!(queue.reserve(), Err(messages::Error::NoSendSlot)));
                            let received = receive(&mut queue, messages, &mut incoming, chunk);
                            let decoded = received.message()?.unwrap();
                            let rpc_capnp::message::Bootstrap(b) = decoded
                                .get_root::<rpc_capnp::message::Reader<'_>>()?
                                .which()
                                .unwrap()
                            else {
                                panic!("Bootstrap")
                            };
                            let answer = answers
                                .admit(AnswerId::from_wire(b?.get_question_id()), None)
                                .unwrap();
                            let mut batch = exports.batch();
                            let grant = batch.export(41u64).unwrap();
                            let bootstrap_export = grant.id();
                            output.reset();
                            {
                                let mut root = output.try_init_root::<Message>()?;
                                let mut result = root.field(Message::RETURN).init()?;
                                result.scalar(Return::ANSWER_ID).set(answer.id().to_wire());
                                result.scalar(Return::RELEASE_PARAM_CAPS).set(false);
                                let mut payload = result.field(Return::RESULTS).init()?;
                                payload.field(Payload::CONTENT).set_capability_index(0)?;
                                payload
                                    .field(Payload::CAP_TABLE)
                                    .init(1)?
                                    .get_mut(0)
                                    .unwrap()
                                    .scalar(CapDescriptor::SENDER_HOSTED)
                                    .set(grant.id().to_wire());
                            }
                            let frame = seal(messages, &output);
                            let original_bootstrap_result = messages.retain(&frame).unwrap();
                            assert!(admission::granting_answer(
                                answers.prepare_return(answer).unwrap(),
                                batch,
                                messages,
                                frame,
                                &mut [0],
                                Default::default()
                            )
                            .unwrap()
                            .accept(queue.reserve().unwrap())
                            .is_none());
                            let received = receive(&mut queue, messages, &mut incoming, chunk);
                            let decoded = received.message()?.unwrap();
                            let rpc_capnp::message::Return(r) = decoded
                                .get_root::<rpc_capnp::message::Reader<'_>>()?
                                .which()
                                .unwrap()
                            else {
                                panic!("Return")
                            };
                            let r = r?;
                            let rpc_capnp::return_::Results(payload) = r.which().unwrap() else {
                                panic!("Results")
                            };
                            let payload = payload?;
                            let index = payload.get_content().capability_index()?;
                            let descriptor = payload.get_cap_table()?.try_get(index).unwrap();
                            let rpc_capnp::cap_descriptor::SenderHosted(id) =
                                descriptor.which().unwrap()
                            else {
                                panic!("senderHosted")
                            };
                            let mut receipts = [imports::ReceiveSlot::new(
                                ImportId::from_wire(id),
                                ImportId::from_wire(id),
                            )];
                            let view = messages.read(&original_bootstrap);
                            let mut bytes = &*view;
                            let original = capnp::serialize::read_message_from_flat_slice_no_alloc(
                                &mut bytes,
                                Default::default(),
                            )?;
                            let (key, reply) = inbound::returned(
                                r,
                                original.get_root()?,
                                questions,
                                exports,
                                imports,
                                &mut receipts,
                                &mut [0],
                            )
                            .unwrap()
                            .accept();
                            assert_eq!(key, bootstrap.key());
                            let inbound::Reply::Results(received_caps) = reply else {
                                panic!("bootstrap result");
                            };
                            drop(view);
                            messages.release(original_bootstrap);
                            let pointer = received_caps
                                .content()
                                .get_as::<capnp::wire_capability::Reader<'_, echo::Owned>>()?;
                            let client = imports
                                .retain(received_caps.capability(pointer).unwrap())
                                .unwrap();
                            received_caps.release(imports);
                            questions.release(bootstrap);
                            send_finish(
                                questions,
                                answers,
                                &mut output,
                                messages,
                                &mut queue,
                                &mut incoming,
                                chunk,
                                exports,
                                Some(&original_bootstrap_result),
                            )?;
                            messages.release(original_bootstrap_result);

                            output.reset();
                            let reservation = questions.reserve().unwrap();
                            calls::call(
                                &mut output,
                                reservation.id(),
                                *imports.get(&client),
                                echo::Owned::ECHO,
                                |mut params| {
                                    params.scalar(EchoParams::VALUE).set(43);
                                    Ok(())
                                },
                            )?;
                            let frame = seal(messages, &output);
                            let original_call = messages.retain(&frame).unwrap();
                            let local = admission::question(
                                reservation,
                                messages,
                                frame,
                                Default::default(),
                            )
                            .unwrap()
                            .accept(queue.reserve().unwrap());
                            let call_key = local.key();
                            let mut local = Some(local);
                            let received = receive(&mut queue, messages, &mut incoming, chunk);
                            let decoded = received.message()?.unwrap();
                            let rpc_capnp::message::Call(c) = decoded
                                .get_root::<rpc_capnp::message::Reader<'_>>()?
                                .which()
                                .unwrap()
                            else {
                                panic!("Call")
                            };
                            let mut receipts = [];
                            let admitted = inbound::call(
                                c?,
                                answers,
                                exports,
                                imports,
                                &mut receipts,
                                &mut [],
                            )
                            .unwrap()
                            .accept(None);
                            let call = admitted.incoming;
                            let pipeline::Target::Hosted(lease) = admitted.target else {
                                panic!("imported target")
                            };
                            assert_eq!(*exports.get(&lease), 41);
                            let answer = admitted.answer;
                            *answers.get_mut(answer).unwrap() = Some(lease);
                            admitted.payload.release(imports);
                            let dispatch = call.dispatch(echo::Owned::ECHO)?;
                            assert!(call.dispatch(echo::Owned::CAPABILITY).is_err());
                            assert!(call
                                .dispatch(portable_capnp::derived_service::Owned::CALL)
                                .is_err());
                            assert_eq!(dispatch.params.value(), 43);
                            // Keep only application data, never a borrowed receive view,
                            // across reuse of the input buffer.
                            let value = dispatch.params.value();
                            let policy = dispatch.allow_cancellation();
                            if cancel {
                                questions.release(local.take().unwrap());
                                send_finish(
                                    questions,
                                    answers,
                                    &mut output,
                                    messages,
                                    &mut queue,
                                    &mut incoming,
                                    chunk,
                                    exports,
                                    None,
                                )?;
                                assert!(!answers.dispatch(answer, policy).unwrap());
                                output.reset();
                                calls::cancelled(&mut output, answer.id())?;
                            } else {
                                assert!(answers.dispatch(answer, policy).unwrap());
                                output.reset();
                                calls::results(
                                    &mut output,
                                    answer.id(),
                                    echo::Owned::ECHO,
                                    |mut results| {
                                        results.scalar(EchoResults::VALUE).set(value + 1);
                                        Ok(())
                                    },
                                )?;
                            }
                            let frame = seal(messages, &output);
                            let original_result = if cancel {
                                None
                            } else {
                                Some(messages.retain(&frame).unwrap())
                            };
                            if let Some(Some(lease)) = admission::answer(
                                answers.prepare_return(answer).unwrap(),
                                messages,
                                frame,
                                Default::default(),
                            )
                            .unwrap()
                            .accept(queue.reserve().unwrap())
                            {
                                assert!(exports.release_local(lease).is_none());
                            }
                            let received = receive(&mut queue, messages, &mut incoming, chunk);
                            let decoded = received.message()?.unwrap();
                            let rpc_capnp::message::Return(r) = decoded
                                .get_root::<rpc_capnp::message::Reader<'_>>()?
                                .which()
                                .unwrap()
                            else {
                                panic!("Return")
                            };
                            let r = r?;
                            if cancel {
                                assert!(matches!(r.which(), Ok(rpc_capnp::return_::Canceled(()))));
                            } else {
                                let rpc_capnp::return_::Results(payload) = r.which().unwrap()
                                else {
                                    panic!("Results")
                                };
                                assert_eq!(
                                    echo::Owned::ECHO
                                        .read_results(payload?.get_content())?
                                        .value(),
                                    44
                                );
                            }
                            let view = messages.read(&original_call);
                            let mut bytes = &*view;
                            let original = capnp::serialize::read_message_from_flat_slice_no_alloc(
                                &mut bytes,
                                Default::default(),
                            )?;
                            let mut receipts = [];
                            let (key, reply) = inbound::returned(
                                r,
                                original.get_root()?,
                                questions,
                                exports,
                                imports,
                                &mut receipts,
                                &mut [0],
                            )
                            .unwrap()
                            .accept();
                            assert_eq!(key, call_key);
                            match reply {
                                inbound::Reply::Results(payload) => {
                                    assert!(!cancel);
                                    payload.release(imports);
                                }
                                inbound::Reply::Discarded => assert!(cancel),
                                inbound::Reply::Exception(_) => panic!("unexpected exception"),
                            }
                            drop(view);
                            messages.release(original_call);
                            if let Some(local) = local {
                                questions.release(local);
                                send_finish(
                                    questions,
                                    answers,
                                    &mut output,
                                    messages,
                                    &mut queue,
                                    &mut incoming,
                                    chunk,
                                    exports,
                                    original_result.as_ref(),
                                )?;
                                messages.release(original_result.unwrap());
                            }
                            assert!(questions.is_empty() && answers.is_empty());

                            imports.release(client);
                            let release = imports.pending_release(0).unwrap();
                            output.reset();
                            calls::release(&mut output, release.id(), release.count())?;
                            let frame = seal(messages, &output);
                            admission::release(release, messages, frame, Default::default())
                                .unwrap()
                                .accept(queue.reserve().unwrap());
                            let received = receive(&mut queue, messages, &mut incoming, chunk);
                            let decoded = received.message()?.unwrap();
                            let rpc_capnp::message::Release(r) = decoded
                                .get_root::<rpc_capnp::message::Reader<'_>>()?
                                .which()
                                .unwrap()
                            else {
                                panic!("Release")
                            };
                            let r = r?;
                            assert_eq!(r.get_id(), bootstrap_export.to_wire());
                            inbound::released(r, exports, &mut [0]).unwrap().accept();
                            assert_eq!(exports.take_retired(0), Some(41));
                            assert!(exports.is_empty() && imports.is_empty() && queue.is_empty());
                            Ok(())
                        },
                    )
                    .unwrap()
                })
                .unwrap()
            })
            .unwrap()
        })
        .unwrap()
    })
    .unwrap()
}

#[allow(clippy::too_many_arguments)]
fn send_finish<'pool, 'exports>(
    questions: &mut questions::Questions<'_, '_>,
    answers: &mut answers::Answers<'_, '_, Option<exports::Lease<'exports>>>,
    output: &mut message::FixedBuilder<'_>,
    messages: &mut messages::Messages<'pool, '_, '_>,
    queue: &mut messages::Queue<'pool, '_>,
    input: &mut [Word],
    chunk: usize,
    exports: &mut exports::Exports<'exports, '_, u64>,
    original: Option<&messages::Lease<'pool>>,
) -> Result<()> {
    let finish = questions.pending_finish().unwrap();
    output.reset();
    calls::finish(output, finish.id(), finish.release_result_caps())?;
    let frame = seal(messages, output);
    admission::finish(finish, messages, frame, Default::default())
        .unwrap()
        .accept(queue.reserve().unwrap());
    let received = receive(queue, messages, input, chunk);
    let decoded = received.message()?.unwrap();
    let rpc_capnp::message::Finish(f) = decoded
        .get_root::<rpc_capnp::message::Reader<'_>>()?
        .which()
        .unwrap()
    else {
        panic!("Finish")
    };
    let f = f?;
    let view = original.map(|lease| messages.read(lease));
    let mut bytes = view.as_deref().unwrap_or(&[]);
    let original = if original.is_some() {
        Some(capnp::serialize::read_message_from_flat_slice_no_alloc(
            &mut bytes,
            Default::default(),
        )?)
    } else {
        None
    };
    let original = original.as_ref().map(|r| r.get_root()).transpose()?;
    let completed = inbound::finished(f, original, answers, exports, &mut [0])
        .unwrap()
        .accept();
    if let Some(Some(lease)) = completed.and_then(|r| r.retired) {
        assert!(exports.release_local(lease).is_none());
    }
    Ok(())
}
