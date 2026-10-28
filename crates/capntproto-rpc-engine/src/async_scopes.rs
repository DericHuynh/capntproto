use crate::{
    answers, exports,
    ids::{AnswerId, ImportId, WireId},
    imports, messages, questions,
};
use core::{
    future::Future,
    task::{Context, Poll, Waker},
};

async fn suspend_once() {
    let mut suspended = false;
    core::future::poll_fn(|cx| {
        if suspended {
            Poll::Ready(())
        } else {
            suspended = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    })
    .await
}
fn run<F: Future>(f: F) -> F::Output {
    let mut future = core::pin::pin!(f);
    let mut cx = Context::from_waker(Waker::noop());
    for _ in 0..8 {
        if let Poll::Ready(value) = future.as_mut().poll(&mut cx) {
            return value;
        }
    }
    panic!("fixture exceeded poll budget");
}

#[test]
fn nested_domains_keep_affine_owners_across_suspension_without_allocation() {
    let measured = allocation_counter::measure(|| {
        let mut export_slots = [exports::Slot::EMPTY];
        run(exports::with_exports_async(
            &mut export_slots,
            async |exports| {
                let mut batch = exports.batch();
                let grant = batch.export(17).unwrap();
                batch.commit();
                let lease = exports.lease(grant.id()).unwrap();
                let mut answer_slots = [answers::Slot::EMPTY];
                answers::with_answers_async(&mut answer_slots, async |answers| {
                    let key = answers.admit(AnswerId::from_wire(9), lease).unwrap();
                    let pipeline = answers.pipeline(key.id()).unwrap();
                    suspend_once().await;
                    assert_eq!(exports.release(grant.id(), 1), Ok(None));
                    assert_eq!(exports.get(answers.pipelined(&pipeline)), &17);
                    answers.finish(key.id(), false);
                    assert!(answers.dispatch(key, false).unwrap());
                    assert!(answers.prepare_return(key).unwrap().accept().is_none());
                    suspend_once().await;
                    let lease = answers.release_pipeline(pipeline).unwrap();
                    assert_eq!(exports.release_local(lease), Some(17));
                })
                .await
                .unwrap();
            },
        ))
        .unwrap();

        let mut question_slots = [questions::Slot::EMPTY];
        run(questions::with_questions_async(
            &mut question_slots,
            async |questions| {
                let local = questions.reserve().unwrap().accept();
                let id = local.key().id();
                suspend_once().await;
                questions.release(local);
                let finish = questions.pending_finish().unwrap();
                suspend_once().await;
                finish.accept();
                questions.returned(id, false).unwrap();
                assert!(questions.is_empty());
            },
        ))
        .unwrap();
    });
    assert_eq!(measured.count_total, 0);
}

#[test]
fn cancelling_a_suspended_scope_detaches_owners_and_releases_message_storage() {
    struct Owner<'a>(&'a core::cell::Cell<u32>);
    impl Drop for Owner<'_> {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let dropped = core::cell::Cell::new(0);
    let mut slots = [imports::Slot::EMPTY];
    {
        let future = imports::with_imports_async(&mut slots, async |imports| {
            let local = imports
                .admit(ImportId::from_wire(7), Owner(&dropped))
                .ok()
                .unwrap();
            suspend_once().await;
            imports.release(local);
        });
        let mut future = core::pin::pin!(future);
        assert!(future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending());
        assert_eq!(dropped.get(), 0);
    }
    assert_eq!(dropped.get(), 1);
    imports::with_imports(&mut slots, |imports| assert!(imports.is_empty())).unwrap();

    let mut bytes = [0u8; 4];
    let mut buffers = [messages::Buffer::new(&mut bytes)];
    let mut sends = [messages::SendSlot::EMPTY];
    {
        let future = messages::with_messages_async(&mut buffers, &mut sends, async |messages| {
            let writer = messages.reserve(4).unwrap();
            messages.write(&writer).copy_from_slice(b"test");
            let lease = messages.seal(writer, 4).unwrap();
            let offer = messages.offer(&lease).unwrap();
            let mut entries = [None];
            let mut queue = messages::Queue::new(&mut entries).unwrap();
            queue.reserve().unwrap().accept(messages, offer);
            messages.release(lease);
            queue.window(messages).unwrap().unwrap().advance(1).unwrap();
            suspend_once().await;
        });
        let mut future = core::pin::pin!(future);
        assert!(future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending());
    }
    // This is new-connection storage reuse; the old partially written stream
    // must be discarded, not resumed with a new frame.
    messages::with_messages(&mut buffers, &mut sends, |messages| {
        let writer = messages.reserve(4).unwrap();
        assert_eq!(&*messages.write(&writer), &[0; 4]);
        messages.discard(writer);
    })
    .unwrap();
}

#[test]
fn connection_scope_preserves_then_detaches_all_domains_on_cancellation() {
    #[derive(Debug)]
    struct Owner<'a>(&'a core::cell::Cell<u32>);
    impl Drop for Owner<'_> {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let measured = allocation_counter::measure(|| {
        let dropped = core::cell::Cell::new(0);
        let mut is = [imports::Slot::EMPTY];
        let mut es = [exports::Slot::EMPTY];
        let mut qs = [questions::Slot::EMPTY];
        let mut as_ = [answers::Slot::EMPTY];
        {
            let future = crate::connection::with_connection_async(
                crate::connection::Storage {
                    imports: &mut is,
                    exports: &mut es,
                    questions: &mut qs,
                    pipeline_questions: &mut [],
                    answers: &mut as_,
                },
                async |connection| {
                    let local = connection
                        .imports
                        .admit(ImportId::from_wire(7), Owner(&dropped))
                        .unwrap();
                    let mut batch = connection.exports.batch();
                    let exported = batch.export(Owner(&dropped)).unwrap();
                    batch.commit();
                    let answer = connection
                        .answers
                        .admit(AnswerId::from_wire(11), Owner(&dropped))
                        .unwrap();
                    let question = connection.questions.reserve().unwrap().accept();
                    suspend_once().await;
                    assert_eq!(connection.imports.wire_id(&local).to_wire(), 7);
                    assert!(connection.exports.resolve(exported.key()).is_some());
                    assert!(connection.answers.get(answer).is_ok());
                    assert!(connection.questions.is_live(question.key()));
                },
            );
            let mut future = core::pin::pin!(future);
            assert!(future
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending());
            assert_eq!(dropped.get(), 0);
        }
        assert_eq!(dropped.get(), 3);
        crate::connection::with_connection(
            crate::connection::Storage {
                imports: &mut is,
                exports: &mut es,
                questions: &mut qs,
                pipeline_questions: &mut [],
                answers: &mut as_,
            },
            |connection| {
                assert!(
                    connection.imports.is_empty()
                        && connection.exports.is_empty()
                        && connection.questions.is_empty()
                        && connection.answers.is_empty()
                );
            },
        )
        .unwrap();
    });
    assert_eq!(measured.count_total, 0);
}
