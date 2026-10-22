//! Parked driver notifications with real invocation and receipt ownership.
use super::{
    driver::with_owned_peer,
    driver_invocation::{close, remote},
    portable_capnp::echo,
    ZERO,
};
use capnp_rpc::{
    bounded::{
        calls,
        owned::{
            connection::{CallProgress, InvocationSlot, Session},
            Frame,
        },
    },
    engine::{
        completion,
        ids::{AnswerId, ImportId, WireId},
        messages,
    },
};
use capntproto_rpc_engine::clients::Capabilities as _;
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};

pub fn caller_polled_session(waker: &Waker, abandon: bool, mut checkpoint: impl FnMut(usize)) {
    let mut words = [[ZERO; 97]; 6];
    let mut buffers = words
        .each_mut()
        .map(|words| messages::Buffer::new(capnp::Word::words_to_bytes_mut(words)));
    super::driver::message_scope(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        let mut sends = [None];
        let mut queue = super::driver::message_queue(&mut sends).unwrap();
        with_owned_peer(|mut peer| {
            let target = remote(&mut peer, ImportId::from_wire(91));
            let slot = completion::Slot::new();
            let mut bindings = [InvocationSlot::EMPTY];
            let session = Session::new(
                peer,
                messages,
                super::driver::invocations(&mut bindings).unwrap(),
                776,
                Default::default(),
            );
            let mut drive = session.drive().unwrap();
            let mut cx = Context::from_waker(waker);
            assert!(drive
                .poll_with(&mut cx, |_, _, _, _| Poll::<()>::Pending)
                .unwrap()
                .is_pending());
            checkpoint(0);
            let reply = session
                .call(&slot, &target, echo::Owned::ECHO, &[], |_| Ok(()))
                .unwrap();
            checkpoint(1);
            let mut question = None;
            assert!(drive
                .poll_with(&mut cx, |driver, messages, calls, cx| {
                    assert_eq!(
                        calls.poll_one(0, driver, messages, cx).unwrap(),
                        CallProgress::Pending
                    );
                    let sent = calls
                        .send_one(
                            driver,
                            messages,
                            &mut queue,
                            776,
                            Default::default(),
                            &mut [0; 4],
                        )
                        .unwrap()
                        .unwrap();
                    question = Some(sent.question);
                    super::owning::exchange(messages, &mut queue, 1).release(messages);
                    Poll::<()>::Pending
                })
                .unwrap()
                .is_pending());
            checkpoint(1);
            session
                .access(|driver, messages, _| {
                    let frame = Frame::build(messages, 776, Default::default(), |builder| {
                        calls::results(
                            builder,
                            AnswerId::from_wire(question.unwrap().id().to_wire()),
                            echo::Owned::ECHO,
                            |_| Ok(()),
                        )
                    })
                    .unwrap();
                    driver
                        .receive(
                            frame,
                            messages,
                            &mut [0; 4],
                            |_| (),
                            || panic!("Return producer"),
                        )
                        .unwrap_or_else(|_| panic!("Return"));
                })
                .unwrap();
            checkpoint(2);
            assert!(drive
                .poll_with(&mut cx, |driver, messages, calls, cx| {
                    assert_eq!(
                        calls.poll_one(0, driver, messages, cx).unwrap(),
                        CallProgress::Delivered
                    );
                    Poll::<()>::Pending
                })
                .unwrap()
                .is_pending());
            checkpoint(2);
            if abandon {
                drop(reply);
                checkpoint(3);
            } else {
                let mut reply = reply;
                let Poll::Ready(Ok(reply)) = Pin::new(&mut reply).poll(&mut cx) else {
                    panic!("delivered reply")
                };
                checkpoint(3);
                assert_eq!(session.read(&reply, |results| results.value()).unwrap(), 0);
                checkpoint(3);
                session.release(reply).unwrap_or_else(|_| panic!("release"));
                checkpoint(4);
            }
            assert_eq!(
                drive
                    .poll_with(&mut cx, |driver, messages, calls, cx| {
                        Poll::Ready(calls.poll_one(0, driver, messages, cx).unwrap())
                    })
                    .unwrap(),
                Poll::Ready(if abandon {
                    CallProgress::Reclaimed
                } else {
                    CallProgress::Consumed
                })
            );
            checkpoint(if abandon { 3 } else { 4 });
            drop(drive);
            drop(slot.try_pair().unwrap());
            let (mut peer, messages, calls) = session.into_parts();
            assert!(calls.is_empty());
            assert_eq!(calls.terminate(&mut peer, messages), 0);
            peer.capabilities().release(target);
            assert_eq!(close(peer, messages, &mut queue), 0);
            let writers = core::array::from_fn::<_, 6, _>(|_| messages.reserve(776).unwrap());
            for writer in writers {
                messages.discard(writer);
            }
        });
    })
    .unwrap();
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::{
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
        task::Wake,
    };
    #[derive(Default)]
    struct Signal(AtomicUsize);
    impl Wake for Signal {
        fn wake(self: Arc<Self>) {
            self.wake_by_ref();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
    #[test]
    fn caller_polled_session_wakes_on_call_input_and_reply_release() {
        for abandon in [false, true] {
            let signal = Arc::new(Signal::default());
            let wake = Waker::from(signal.clone());
            caller_polled_session(&wake, abandon, |count| {
                assert_eq!(signal.0.load(Ordering::Relaxed), count)
            });
        }
    }
}
