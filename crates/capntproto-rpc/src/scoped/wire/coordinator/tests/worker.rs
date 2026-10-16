use super::*;
use capntproto_rpc_engine::ids::QuestionId;
use capntproto_rpc_wire::{calls, rpc_capnp};

type Application =
    dyn Fn(capability::Results<any_pointer::Owned>) -> capability::DispatchCallResult;
#[derive(Clone)]
struct Service(Rc<Application>);
impl capability::Server for Service {
    fn as_ptr(&self) -> usize {
        Rc::as_ptr(&self.0) as *const () as usize
    }
    fn dispatch_call(
        self,
        _: u64,
        _: u16,
        _: capability::Params<any_pointer::Owned>,
        results: capability::Results<any_pointer::Owned>,
    ) -> capability::DispatchCallResult {
        (self.0)(results)
    }
}
fn service(
    f: impl Fn(capability::Results<any_pointer::Owned>) -> capability::DispatchCallResult + 'static,
) -> Rc<dyn ClientHook> {
    Rc::new(crate::local::Client::new(Service(Rc::new(f))))
}

#[test]
fn pending_worker_wake_dispatches_early_pipeline_before_parent_completes() {
    for budget in [1, 7, 64] {
        with_peer(|mut driver, messages| {
            let child = service(|mut results| {
                results.get().set_as::<capnp::text::Owned>("child").unwrap();
                capability::DispatchCallResult::new(capability::Promise::ok(()), false)
            });
            let (early, early_receiver) = shared::channel::<()>();
            let (finish, finish_receiver) = shared::channel::<()>();
            let receivers = RefCell::new(Some((early_receiver, finish_receiver)));
            let target = expose(
                &mut driver,
                service(move |mut results| {
                    let child = child.add_ref();
                    let (early, finish) = receivers.borrow_mut().take().unwrap();
                    capability::DispatchCallResult::new(
                        capability::Promise::from_future(async move {
                            early.await.map_err(crate::canceled_to_error)?;
                            results.get().set_as_capability(child);
                            results.set_pipeline()?;
                            finish.await.map_err(crate::canceled_to_error)?;
                            Err(Error::overloaded("parent completed after child".into()))
                        }),
                        false,
                    )
                }),
            );
            for (id, target) in [
                (0, calls::Target::Imported(target)),
                (
                    1,
                    calls::Target::Promised {
                        question: QuestionId::from_wire(0),
                        path: pipeline::Path::new(&[]),
                    },
                ),
            ] {
                let frame = Frame::build(messages, 512, Default::default(), |root| {
                    calls::dynamic_call(
                        root,
                        QuestionId::from_wire(id),
                        target,
                        DynamicCall {
                            interface: 7,
                            ..Default::default()
                        },
                        |_| Ok(()),
                    )
                })
                .unwrap();
                driver
                    .receive(frame, messages, &mut [0; 16], |_| (), server::Results::new)
                    .unwrap_or_else(|rejected| panic!("{}", rejected.error));
            }
            let mut opts = options();
            opts.budget = NonZeroUsize::new(budget).unwrap();
            let mut coordinator = Coordinator::new(
                driver,
                ClientHooks::new(64, 16),
                NonZeroUsize::new(2).unwrap(),
                None,
                opts,
            )
            .unwrap();
            let mut queue = messages::Queue::new_growing(0, 4).unwrap();
            let parent = Arc::new(WakeCount::default());
            let parent_waker = Waker::from(parent.clone());
            let mut cx = Context::from_waker(&parent_waker);
            let mut scratch = [0; 32];
            for _ in 0..512 {
                if let Poll::Ready(result) =
                    coordinator.poll_work(messages, &mut queue, &mut scratch, &NoResources, &mut cx)
                {
                    result.unwrap();
                }
                if coordinator.active == 0 && coordinator.wake.changed.load(Ordering::Acquire) == 0
                {
                    break;
                }
            }
            assert_eq!(coordinator.active, 0);
            assert!(queue.is_empty());
            let wakes = parent.0.load(Ordering::SeqCst);
            early.send(()).unwrap();
            assert!(parent.0.load(Ordering::SeqCst) > wakes);
            assert_ne!(coordinator.wake.changed.load(Ordering::Acquire), 0);
            for _ in 0..1024 {
                if let Poll::Ready(result) =
                    coordinator.poll_work(messages, &mut queue, &mut scratch, &NoResources, &mut cx)
                {
                    result.unwrap();
                }
                if !queue.is_empty() {
                    break;
                }
            }
            {
                let window = queue
                    .window(messages)
                    .unwrap()
                    .expect("early child reply while parent waits");
                let mut bytes = window.bytes();
                let reader = capnp::serialize::read_message_from_flat_slice_no_alloc(
                    &mut bytes,
                    Default::default(),
                )
                .unwrap();
                let root = reader.get_root::<rpc_capnp::message::Reader>().unwrap();
                let rpc_capnp::message::Return(reply) = root.which().unwrap() else {
                    panic!("Return")
                };
                let reply = reply.unwrap();
                assert_eq!(reply.get_answer_id(), 1);
                let rpc_capnp::return_::Results(result) = reply.which().unwrap() else {
                    panic!("child results")
                };
                assert_eq!(
                    result
                        .unwrap()
                        .get_content()
                        .get_as::<capnp::text::Reader>()
                        .unwrap(),
                    "child"
                );
                let count = window.bytes().len();
                window.advance(count).unwrap();
            }
            finish.send(()).unwrap();
            for _ in 0..1024 {
                if let Poll::Ready(result) =
                    coordinator.poll_work(messages, &mut queue, &mut scratch, &NoResources, &mut cx)
                {
                    result.unwrap();
                }
                if !queue.is_empty() {
                    break;
                }
            }
            assert!(!queue.is_empty(), "parent completion wakes independently");
            futures::executor::block_on(coordinator.terminate::<NoResources, (), ()>(
                messages,
                &mut queue,
                &mut Policy::default(),
                Ok(()),
            ))
            .unwrap();
            reclaimed(messages);
        });
    }
}
