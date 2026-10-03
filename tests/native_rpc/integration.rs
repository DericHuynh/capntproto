use super::super::*;
use capnp::{
    capability::Promise,
    private::capability::{ClientHook, ParamsHook, PipelineHook, PipelineOp, ResultsHook},
};
use std::{
    cell::RefCell,
    panic::{catch_unwind, AssertUnwindSafe},
};

#[derive(Default)]
struct Counters {
    clones: Cell<usize>,
    drops: Cell<usize>,
}
struct Opaque(Rc<Counters>);
impl Drop for Opaque {
    fn drop(&mut self) {
        self.0.drops.set(self.0.drops.get() + 1);
    }
}
impl ClientHook for Opaque {
    fn add_ref(&self) -> Box<dyn ClientHook> {
        self.0.clones.set(self.0.clones.get() + 1);
        Box::new(Self(self.0.clone()))
    }
    fn new_call(
        &self,
        _: u64,
        _: u16,
        _: Option<capnp::MessageSize>,
    ) -> capnp::capability::Request<any_pointer::Owned, any_pointer::Owned> {
        panic!("unexpected call")
    }
    fn call(
        &self,
        _: u64,
        _: u16,
        _: Box<dyn ParamsHook>,
        _: Box<dyn ResultsHook>,
    ) -> Promise<(), capnp::Error> {
        panic!("unexpected call")
    }
    fn get_brand(&self) -> usize {
        panic!("unexpected brand")
    }
    fn get_ptr(&self) -> usize {
        panic!("unexpected identity")
    }
    fn get_resolved(&self) -> Option<Box<dyn ClientHook>> {
        panic!("unexpected resolution")
    }
    fn when_more_resolved(&self) -> Option<Promise<Box<dyn ClientHook>, capnp::Error>> {
        panic!("unexpected resolution")
    }
    fn when_resolved(&self) -> Promise<(), capnp::Error> {
        panic!("unexpected resolution")
    }
}
fn opaque(counters: &Rc<Counters>) -> capnp::capability::Client {
    capnp::capability::Client::new(Box::new(Opaque(counters.clone())))
}

#[test]
fn moving_capabilities_preserves_hooks_without_cloning_or_polling_and_failure_is_recoverable() {
    for mode in [false, true] {
        let counters = Rc::new(Counters::default());
        let owned = {
            let loader = loader(true);
            let source = if mode {
                Source::CompiledClient(compiled::Client::new(
                    opaque(&counters),
                    service::Client::<Text>::schema(),
                ))
            } else {
                Source::LoadedClient(
                    loaded::Client::new(
                        opaque(&counters),
                        loader.get(id::<service::Owned<Text>>()).unwrap(),
                    )
                    .unwrap(),
                )
            };
            let source = match release(source, 3) {
                Err((_, s)) => *s,
                Ok(_) => panic!(),
            };
            assert_eq!((counters.clones.get(), counters.drops.get()), (0, 0));
            let shared = share(&source, 2).unwrap();
            assert_eq!(counters.clones.get(), 1);
            drop(shared);
            success(release(source, 2))
        };
        assert_eq!((counters.clones.get(), counters.drops.get()), (1, 1));
        drop(owned);
        assert_eq!(counters.drops.get(), 2);
    }
}
struct OpaquePipeline(Rc<Counters>);
impl Drop for OpaquePipeline {
    fn drop(&mut self) {
        self.0.drops.set(self.0.drops.get() + 1);
    }
}
impl PipelineHook for OpaquePipeline {
    fn add_ref(&self) -> Box<dyn PipelineHook> {
        panic!("native pipeline transfer must not clone")
    }
    fn get_pipelined_cap(&self, _: &[PipelineOp]) -> Box<dyn ClientHook> {
        panic!("native pipeline transfer must not project")
    }
}
#[test]
fn pipeline_transfer_keeps_the_hook_without_projection_or_cloning() {
    for mode in [false, true] {
        let counters = Rc::new(Counters::default());
        let native = {
            let loader = loader(true);
            let raw = any_pointer::Pipeline::new(Box::new(OpaquePipeline(counters.clone())));
            let source = if mode {
                Source::CompiledPipeline(compiled::Pipeline::new(
                    raw,
                    Outer::introspect().as_struct_schema().unwrap(),
                ))
            } else {
                Source::LoadedPipeline(
                    loaded::Pipeline::new(raw, loader.get(id::<Outer>()).unwrap()).unwrap(),
                )
            };
            let source = match release(source, 7) {
                Err((_, s)) => *s,
                Ok(_) => panic!(),
            };
            assert_eq!(counters.drops.get(), 0);
            success(release(source, 6))
        };
        assert_eq!(counters.drops.get(), 0);
        drop(native);
        assert_eq!(counters.drops.get(), 1);
    }
}

#[test]
fn compiled_capability_lists_reject_other_interfaces_inheritance_and_missing_metadata() {
    macro_rules! check {
        ($element:ty,$target:ty,$ok:expr) => {{
            let mut message = capnp::message::Builder::new_default();
            let mut list = message.initn_root::<capnp::capability_list::Builder<$element>>(0);
            let reader: capnp::dynamic_value::Reader = list.reborrow().into_reader().into();
            assert_eq!(
                catch_unwind(AssertUnwindSafe(
                    || reader.downcast::<capnp::capability_list::Reader<$target>>()
                ))
                .is_ok(),
                $ok
            );
            let builder: capnp::dynamic_value::Builder = list.into();
            assert_eq!(
                catch_unwind(AssertUnwindSafe(
                    || builder.downcast::<capnp::capability_list::Builder<$target>>()
                ))
                .is_ok(),
                $ok
            );
        }};
    }
    check!(service::Client<Text>, service::Client<Data>, true);
    check!(service::Client<Text>, other::Client, false);
    check!(derived::Client<Text>, service::Client<Text>, false);
    check!(capnp::capability::Client, service::Client<Text>, false);
    check!(service::Client<Text>, capnp::capability::Client, false);
    check!(capnp::capability::Client, capnp::capability::Client, true);
    // The outer list must not erase the interface before the inner cast runs.
    let mut message = capnp::message::Builder::new_default();
    type Inner = capnp::capability_list::Owned<service::Client<Text>>;
    type Wrong = capnp::capability_list::Owned<other::Client>;
    let mut nested = message.initn_root::<capnp::list_list::Builder<Inner>>(0);
    let reader: capnp::dynamic_value::Reader = nested.reborrow().into_reader().into();
    assert!(catch_unwind(AssertUnwindSafe(
        || reader.downcast::<capnp::list_list::Reader<Wrong>>()
    ))
    .is_err());
    let builder: capnp::dynamic_value::Builder = nested.into();
    assert!(catch_unwind(AssertUnwindSafe(
        || builder.downcast::<capnp::list_list::Builder<Wrong>>()
    ))
    .is_err());
    let deep = |base| (0..1024).fold(base, |ty, _| capnp::introspect::Type::list_of(ty));
    assert!(deep(service::Owned::<Text>::introspect())
        .loose_equals(deep(service::Owned::<Data>::introspect())));
    assert!(
        !deep(service::Owned::<Text>::introspect()).loose_equals(deep(other::Owned::introspect()))
    );
}

#[test]
fn loaded_null_and_untyped_clients_keep_explicit_cast_boundaries() {
    let loader = loader(true);
    let null = loaded::Client::null(Some(loader.get(id::<service::Owned<Text>>()).unwrap()));
    assert!(null.cast_native::<service::Client<Text>>().is_err());
    assert!(null.release_native::<service::Client<Text>>().is_err());
    let counters = Rc::new(Counters::default());
    let dynamic = loaded::Client::new(
        opaque(&counters),
        loader.get(id::<service::Owned<Text>>()).unwrap(),
    )
    .unwrap();
    let typeless = success(dynamic.release_native::<capnp::capability::Client>());
    assert_eq!(counters.clones.get(), 0);
    drop(typeless);
    assert_eq!(counters.drops.get(), 1);
}

struct DelayedFactory {
    gate: RefCell<Option<futures::channel::oneshot::Receiver<()>>>,
    started: Rc<Cell<bool>>,
    cap: service::Client<Text>,
}
impl factory::Server for DelayedFactory {
    async fn open(
        self: Rc<Self>,
        _: factory::OpenParams,
        mut results: factory::OpenResults,
    ) -> capnp::Result<()> {
        self.started.set(true);
        let gate = self.gate.borrow_mut().take().unwrap();
        gate.await
            .map_err(|_| capnp::Error::failed("gate gone".into()))?;
        results
            .get()
            .init_inner()
            .init_body()
            .set_cap(self.cap.clone())?;
        Ok(())
    }
}
struct Drivers(Vec<tokio::task::JoinHandle<capnp::Result<()>>>);
impl Drop for Drivers {
    fn drop(&mut self) {
        for d in &self.0 {
            d.abort();
        }
    }
}
struct Boundary {
    deny: bool,
}
impl capnp_rpc::membrane::Policy for Boundary {
    fn call(
        &self,
        _: capnp_rpc::membrane::Direction,
        interface: u64,
        _: u16,
        _: &capnp::capability::Client,
    ) -> capnp::Result<Option<capnp::capability::Client>> {
        if self.deny && interface == id::<service::Owned<Text>>() {
            Err(capnp::Error::failed("native call denied".into()))
        } else {
            Ok(None)
        }
    }
}
#[tokio::test(flavor = "current_thread")]
async fn converted_pending_pipelines_keep_nested_paths_and_membrane_policy_locally_and_over_rpc() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(std::time::Duration::from_secs(10), async {
                for mode in [false, true] {
                    for wire in [false, true] {
                        for deny in [false, true] {
                            let started = Rc::new(Cell::new(false));
                            let calls = Rc::new(Cell::new(0));
                            let alive = Rc::new(Cell::new(false));
                            alive.set(true);
                            let target: service::Client<Text> = capnp_rpc::new_client(Echo {
                                alive,
                                calls: calls.clone(),
                            });
                            let (tx, rx) = futures::channel::oneshot::channel();
                            let factory: factory::Client = capnp_rpc::new_client(DelayedFactory {
                                gate: RefCell::new(Some(rx)),
                                started: started.clone(),
                                cap: target,
                            });
                            let membrane =
                                capnp_rpc::membrane::Membrane::new(Rc::new(Boundary { deny }));
                            let factory = membrane.export(factory);
                            let mut drivers = Drivers(Vec::new());
                            let factory = if wire {
                                let (a, b) = tokio::io::duplex(4096);
                                drivers.0.push(capntproto::rpc::serve(b, factory.client));
                                let (c, d) = capntproto::rpc::client::<factory::Client>(a);
                                drivers.0.push(d);
                                c
                            } else {
                                factory
                            };
                            let typed = {
                                let loader = loader(true);
                                let source = if mode {
                                    Source::CompiledPipeline(
                                        compiled::Client::new(factory, factory::Client::schema())
                                            .new_request("open", None)
                                            .unwrap()
                                            .send_for_pipeline(),
                                    )
                                } else {
                                    Source::LoadedPipeline(
                                        loaded::Client::new(
                                            factory,
                                            loader.get(id::<factory::Owned>()).unwrap(),
                                        )
                                        .unwrap()
                                        .new_request("open")
                                        .unwrap()
                                        .send_for_pipeline()
                                        .unwrap(),
                                    )
                                };
                                let source = match release(source, 7) {
                                    Err((_, s)) => *s,
                                    Ok(_) => panic!(),
                                };
                                success(release(source, 6))
                            };
                            // Native pipeline and its child outlive the loader, before results exist.
                            let Native::Changed(pipeline) = typed else {
                                panic!()
                            };
                            let cap = pipeline.get_inner().get_body().get_cap();
                            let mut request = cap.ping_request();
                            request.get().set_value(41);
                            let mut pending = Box::pin(request.send().promise);
                            assert!(futures::poll!(&mut pending).is_pending());
                            for _ in 0..32 {
                                tokio::task::yield_now().await;
                            }
                            assert!(started.get());
                            assert_eq!(calls.get(), 0);
                            tx.send(()).unwrap();
                            let response = pending.await;
                            if deny {
                                assert!(response
                                    .err()
                                    .unwrap()
                                    .to_string()
                                    .contains("native call denied"));
                            } else {
                                assert_eq!(response.unwrap().get().unwrap().get_value(), 42);
                                assert_eq!(calls.get(), 1);
                            }
                            membrane
                                .revoke(capnp::Error::failed("revoked native capability".into()));
                            assert!(cap.ping_request().send().promise.await.is_err());
                            drop(pipeline);
                            drop(cap);
                            drop(drivers);
                        }
                    }
                }
            })
            .await
            .unwrap();
        })
        .await;
}
