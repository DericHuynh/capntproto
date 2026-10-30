//! Exact wake ownership for incoming network callbacks. The immutable request
//! and producer stay in a reusable slot until publication or explicit teardown.
use super::*;
use crate::scoped::wire::ready;

struct Entry<'p, 'id, const H: usize> {
    job: Incoming<'p, 'id, H>,
    producer: server::Results,
}
#[derive(Default)]
pub(super) struct Pending<'p, 'id, const H: usize> {
    owners: ready::Owners<Entry<'p, 'id, H>>,
}
impl<'p, 'id, const H: usize> Pending<'p, 'id, H> {
    pub(super) fn len(&self) -> usize {
        self.owners.len()
    }
    pub(super) fn is_empty(&self) -> bool {
        self.owners.is_empty()
    }
    pub(super) fn try_reserve(
        &mut self,
        additional: usize,
    ) -> Result<(), std::collections::TryReserveError> {
        self.owners.try_reserve(additional)
    }
    pub(super) fn push_back(&mut self, job: Incoming<'p, 'id, H>, producer: server::Results) {
        let index = self.owners.insert(Entry { job, producer }, true);
        let (entry, waker) = self.owners.get_mut(index);
        entry.producer.bind_protocol_waker(waker.clone());
    }
    pub(super) fn poll(
        &mut self,
        cx: &mut Context<'_>,
        available: bool,
        canceled: impl FnMut(capntproto_rpc_engine::answers::Key<'id>) -> bool,
    ) -> Poll<Incoming<'p, 'id, H>> {
        if self.is_empty() {
            Poll::Pending
        } else {
            self.poll_active(cx, available, canceled)
        }
    }
    pub(super) fn drain(&mut self) -> impl Iterator<Item = Incoming<'p, 'id, H>> {
        self.owners.drain().map(|entry| {
            entry.producer.unbind_protocol_waker();
            entry.job
        })
    }
    #[inline(never)]
    fn poll_active(
        &mut self,
        cx: &mut Context<'_>,
        available: bool,
        mut canceled: impl FnMut(capntproto_rpc_engine::answers::Key<'id>) -> bool,
    ) -> Poll<Incoming<'p, 'id, H>> {
        let mut visited = 0u128;
        // Bound a turn, including overflow. Coalesced self-wakes receive one
        // visit per rotation; completed output waits for IO without spinning.
        for _ in 0..self.len() {
            let Some(ready) = self.owners.next(cx) else {
                break;
            };
            let index = ready.index();
            if index < 128 {
                let bit = 1u128 << index;
                if visited & bit != 0 {
                    if available {
                        self.owners.notify(index);
                    } else {
                        self.owners.schedule(index);
                    }
                    break;
                }
                visited |= bit;
            }
            let (entry, waker) = self.owners.get_mut(index);
            let key = entry.job.request.answer_key().expect("answer control");
            if canceled(key) && entry.job.future.is_some() {
                entry.job.future = None;
                entry.job.completed = Some(Err(closed()));
            }
            if let Some(future) = &mut entry.job.future {
                if let Poll::Ready(result) = Pin::new(future).poll(&mut Context::from_waker(waker))
                {
                    entry.job.future = None;
                    entry.job.completed = Some(result);
                }
            }
            if entry.job.completed.is_some() {
                if available {
                    let entry = self.owners.take(index);
                    entry.producer.unbind_protocol_waker();
                    return Poll::Ready(entry.job);
                }
                self.owners.schedule(index);
            }
        }
        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use capntproto_rpc_engine::{
        connection as storage,
        ids::{QuestionId, WireId},
    };
    use capntproto_rpc_wire::{calls, network, owned::Frame};

    #[test]
    fn pending_network_callbacks_keep_exact_readiness_finish_and_shutdown_ownership() {
        const LEDGERS: usize = if cfg!(miri) { 4 } else { 256 };
        let counts: &[usize] = if cfg!(miri) { &[3] } else { &[32, 129] };
        for &count in counts {
            let mut buffers = messages::GrowingBuffers::new(count + 1, 1024, (count + 1) * 1024);
            messages::with_growing_messages(
                &mut buffers,
                storage::Growth {
                    initial: 0,
                    maximum: count + 1,
                },
                |messages| {
                    storage::with_domain(|domain| {
                        let mut storage = connection::FixedStorage::<
                            (),
                            server::Producer,
                            u8,
                            Rc<dyn ClientHook>,
                            0,
                            0,
                            4,
                            4,
                            0,
                            LEDGERS,
                        >::new();
                        let driver = storage.bind(domain, 1).unwrap();
                        let mut coordinator = Coordinator::new(
                            driver,
                            ClientHooks::new(4, 4),
                            NonZeroUsize::new(4).unwrap(),
                            None,
                            Options {
                                input_bytes: 1024.try_into().unwrap(),
                                output_bytes: 128.try_into().unwrap(),
                                budget: 7.try_into().unwrap(),
                                reader: Default::default(),
                            },
                        )
                        .unwrap();
                        let mut pending = Pending::default();
                        let polls = Rc::new(RefCell::new(vec![0; count]));
                        let wakes = Rc::new(RefCell::new(vec![None::<Waker>; count]));
                        let runnable = Rc::new(RefCell::new(vec![false; count]));
                        for index in 0..count {
                            let frame =
                                Frame::build(messages, 128, Default::default(), |builder| {
                                    network::accept(
                                        builder,
                                        QuestionId::from_wire(index as u32),
                                        None,
                                        |_| Ok(()),
                                    )
                                })
                                .unwrap();
                            assert!(matches!(
                                coordinator
                                    .driver
                                    .receive(
                                        frame,
                                        messages,
                                        &mut [0; LEDGERS],
                                        |_| (),
                                        server::Producer::new
                                    )
                                    .unwrap_or_else(|rejected| panic!("{}", rejected.error)),
                                connection::Received::Network
                            ));
                            let request = coordinator.driver.take_network().unwrap();
                            let key = request.answer_key().unwrap();
                            let producer = coordinator
                                .driver
                                .ledgers()
                                .answers
                                .get(key)
                                .unwrap()
                                .owner
                                .producer
                                .results();
                            let polls = polls.clone();
                            let wakes = wakes.clone();
                            let runnable = runnable.clone();
                            let future = Promise::from_future(std::future::poll_fn(move |cx| {
                                polls.borrow_mut()[index] += 1;
                                wakes.borrow_mut()[index] = Some(cx.waker().clone());
                                if runnable.borrow()[index] {
                                    Poll::Ready(Err(Error::failed(format!(
                                        "network callback {index}"
                                    ))))
                                } else {
                                    Poll::Pending
                                }
                            }));
                            pending.try_reserve(1).unwrap();
                            pending.push_back(
                                Incoming {
                                    request,
                                    future: Some(future),
                                    completed: None,
                                },
                                producer,
                            );
                        }
                        let mut cx = Context::from_waker(Waker::noop());
                        assert!(pending.poll(&mut cx, true, |_| false).is_pending());
                        assert_eq!(*polls.borrow(), vec![1; count]);
                        for _ in 0..8 {
                            assert!(pending
                                .poll(&mut cx, true, |_| panic!(
                                    "idle callback rechecked its ledger"
                                ))
                                .is_pending());
                        }
                        assert_eq!(*polls.borrow(), vec![1; count]);
                        runnable.borrow_mut()[0] = true;
                        wakes.borrow()[0].as_ref().unwrap().wake_by_ref();
                        // A completed callback waits for FIFO capacity without
                        // rebuilding it or letting repeated blocked polls spin.
                        assert!(pending.poll(&mut cx, false, |_| false).is_pending());
                        assert!(pending.poll(&mut cx, false, |_| false).is_pending());
                        let Poll::Ready(job) = pending.poll(&mut cx, true, |_| false) else {
                            panic!("ready callback")
                        };
                        assert_eq!(job.request.answer_key().unwrap().id().to_wire(), 0);
                        assert_eq!(
                            job.completed.unwrap().err().unwrap().extra,
                            "network callback 0"
                        );
                        assert_eq!(polls.borrow()[0], 2);
                        assert!(polls.borrow()[1..].iter().all(|&n| n == 1));
                        coordinator.driver.release_network(job.request, messages);
                        let id = (count - 1) as u32;
                        let frame = Frame::build(messages, 128, Default::default(), |builder| {
                            calls::finish(builder, QuestionId::from_wire(id), true)
                        })
                        .unwrap();
                        let connection::Received::Finished { question, producer } = coordinator
                            .driver
                            .receive(
                                frame,
                                messages,
                                &mut [0; LEDGERS],
                                |_| (),
                                || panic!("Finish must not create a producer"),
                            )
                            .unwrap_or_else(|rejected| panic!("{}", rejected.error))
                        else {
                            panic!("Finish")
                        };
                        assert_eq!(question.id().to_wire(), id);
                        ControlContext::new(&mut coordinator).notify_finished(question, producer);
                        let Poll::Ready(job) = pending.poll(&mut cx, true, |key| {
                            coordinator
                                .driver
                                .ledgers()
                                .answers
                                .cancellation_requested(key)
                                .unwrap_or(true)
                        }) else {
                            panic!("Finish stranded its callback")
                        };
                        assert_eq!(job.request.answer_key().unwrap().id().to_wire(), id);
                        assert!(job.completed.unwrap().is_err());
                        assert_eq!(
                            polls.borrow()[count - 1],
                            1,
                            "cancellation polled its future"
                        );
                        coordinator.driver.release_network(job.request, messages);
                        for job in pending.drain() {
                            coordinator.driver.release_network(job.request, messages);
                        }
                        assert!(pending.is_empty());
                        for wake in wakes.borrow().iter().flatten() {
                            wake.wake_by_ref();
                        }
                        assert!(pending
                            .poll(&mut cx, true, |_| panic!("retired operation"))
                            .is_pending());
                        let mut queue = messages::Queue::new_growing(0, 1).unwrap();
                        futures::executor::block_on(coordinator.terminate::<NoResources, (), ()>(
                            messages,
                            &mut queue,
                            &mut TwoParty::new(
                                || Err(Error::failed("unused bootstrap".into())),
                                1.try_into().unwrap(),
                            ),
                            Ok(()),
                        ))
                        .unwrap();
                        let writers: Vec<_> = (0..count + 1)
                            .map(|_| messages.reserve_exact(128).unwrap())
                            .collect();
                        for writer in writers {
                            messages.discard(writer);
                        }
                    });
                },
            )
            .unwrap();
        }
    }
    #[test]
    fn unused_incoming_network_scheduler_requires_no_allocation() {
        let measured = allocation_counter::measure(|| {
            let mut pending = Pending::<0>::default();
            assert!(pending.is_empty());
            assert!(pending
                .poll(&mut Context::from_waker(Waker::noop()), true, |_| panic!(
                    "no callback"
                ))
                .is_pending());
            assert_eq!(pending.drain().count(), 0);
        });
        assert_eq!(measured.count_total, 0);
    }
}
