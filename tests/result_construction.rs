#[allow(dead_code)]
mod support;
use capnp::{
    capability::{FromClientHook, Promise},
    dynamic_orphan::{Orphan, Root},
    dynamic_value,
    introspect::Introspect,
    message,
    traits::ImbueMut,
    Error, MessageSize,
};
use futures::{channel::oneshot, FutureExt};
use reproto_test_support::runtime_test_capnp::harness;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
mod result_construction {
    pub mod dynamic;
    pub mod verification;
}

struct Echo(u32, Rc<Cell<u32>>);
impl Echo {
    fn client(value: u32, live: &Rc<Cell<u32>>) -> harness::Client {
        live.set(live.get() + 1);
        capnp_rpc::new_client(Self(value, live.clone()))
    }
}
impl Drop for Echo {
    fn drop(&mut self) {
        self.1.set(self.1.get() - 1);
    }
}
impl harness::Server for Echo {
    async fn echo(
        self: Rc<Self>,
        _: harness::EchoParams,
        mut r: harness::EchoResults,
    ) -> capnp::Result<()> {
        r.get().set_value(self.0);
        Ok(())
    }
}
fn result_type() -> capnp::introspect::Type {
    harness::pending_results::Owned::introspect()
}
fn new_orphan<'m>(
    root: &mut Root<'_>,
    token: &capnp::dynamic_orphan::Orphanage<'m>,
    value: u32,
    live: &Rc<Cell<u32>>,
) -> Orphan<'m> {
    let mut access = token.in_root(root).unwrap();
    let orphan = access
        .new_struct(result_type().as_struct_schema().unwrap())
        .unwrap();
    let mut orphan = orphan
        .release_as::<harness::pending_results::Owned>()
        .unwrap();
    access
        .edit_typed(&mut orphan, |mut value_builder| {
            value_builder.set_cap(Echo::client(value, live));
            Ok(())
        })
        .unwrap();
    orphan.into_dynamic()
}
fn root_cap(root: &Root<'_>) -> Option<harness::Client> {
    if root.is_null() {
        return None;
    }
    let value = root
        .as_reader()
        .unwrap()
        .downcast::<capnp::dynamic_struct::Reader>();
    Some(
        value
            .downcast::<harness::pending_results::Owned>()
            .get_cap()
            .unwrap(),
    )
}
async fn value(cap: Option<harness::Client>) -> u32 {
    match cap {
        Some(cap) => cap
            .echo_request()
            .send()
            .promise
            .await
            .unwrap()
            .get()
            .unwrap()
            .get_value(),
        None => 0,
    }
}

struct Construct {
    gate: RefCell<Option<oneshot::Receiver<()>>>,
    live: Rc<Cell<u32>>,
    hint: Option<MessageSize>,
    ready: Rc<Cell<bool>>,
    fail: bool,
}
impl harness::Server for Construct {
    async fn pending(
        self: Rc<Self>,
        _: harness::PendingParams,
        mut results: harness::PendingResults,
    ) -> capnp::Result<()> {
        {
            let (mut root, token) = results.get_orphanage(self.hint)?;
            assert!(root.is_null()); // No throwaway result struct before adoption.
            let orphan = new_orphan(&mut root, &token, 73, &self.live);
            root.adopt(orphan).unwrap();
            let original = root_cap(&root).unwrap().as_client_hook().get_ptr();
            let orphan = root.disown(&token)?;
            assert!(root.is_null());
            root.adopt(orphan).unwrap();
            assert_eq!(
                root_cap(&root).unwrap().as_client_hook().get_ptr(),
                original
            );
        }
        assert_eq!(self.live.get(), 1);
        // Later allocation hints do not replace the arena or invalidate caps.
        results.get_with_size_hint(Some(MessageSize {
            word_count: 1,
            cap_count: 0,
        }));
        results.set_pipeline()?;
        self.ready.set(true);
        let gate = self.gate.borrow_mut().take().unwrap();
        gate.await.map_err(|_| Error::failed("gate lost".into()))?;
        if self.fail {
            Err(Error::failed("late error".into()))
        } else {
            Ok(())
        }
    }
}
struct Drivers(Vec<tokio::task::JoinHandle<capnp::Result<()>>>);
impl Drop for Drivers {
    fn drop(&mut self) {
        for t in &self.0 {
            t.abort();
        }
    }
}
async fn settle() {
    for _ in 0..64 {
        tokio::task::yield_now().await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn result_adoption_keeps_capabilities_through_pipeline_return_errors_and_membranes() {
    use capnp_rpc::{
        membrane::{Direction, Membrane, Policy},
        RpcSystem,
    };
    struct Boundary;
    impl Policy for Boundary {
        fn call(
            &self,
            _: Direction,
            _: u64,
            _: u16,
            _: &capnp::capability::Client,
        ) -> capnp::Result<Option<capnp::capability::Client>> {
            Ok(None)
        }
    }
    tokio::task::LocalSet::new()
        .run_until(async {
            for wire in [false, true] {
                for wrapped in [false, true] {
                    for fail in [false, true] {
                        let live = Rc::new(Cell::new(0));
                        let ready = Rc::new(Cell::new(false));
                        let (gate, rx) = oneshot::channel();
                        let client: harness::Client = capnp_rpc::new_client(Construct {
                            gate: RefCell::new(Some(rx)),
                            live: live.clone(),
                            ready: ready.clone(),
                            hint: Some(MessageSize {
                                word_count: 64,
                                cap_count: 1,
                            }),
                            fail,
                        });
                        let membrane = Membrane::new(Rc::new(Boundary));
                        let client = if wrapped {
                            membrane.export(client)
                        } else {
                            client
                        };
                        let mut drivers = Drivers(Vec::new());
                        let hub = Rc::new(RefCell::new(support::Hub::default()));
                        let client = if wire {
                            let host = RpcSystem::new(
                                Box::new(support::Hub::network(&hub, 1)),
                                Some(client.client),
                            );
                            let mut caller =
                                RpcSystem::new(Box::new(support::Hub::network(&hub, 2)), None);
                            let client = caller.bootstrap(1);
                            drivers.0.push(tokio::task::spawn_local(host));
                            drivers.0.push(tokio::task::spawn_local(caller));
                            client
                        } else {
                            client
                        };
                        let mut call = client.pending_request().send();
                        for _ in 0..64 {
                            assert!((&mut call.promise).now_or_never().is_none());
                            tokio::task::yield_now().await;
                        }
                        assert!(ready.get());
                        let cap = call.pipeline.get_cap();
                        assert_eq!(value(Some(cap.clone())).await, 73);
                        assert_eq!(live.get(), 1);
                        gate.send(()).unwrap();
                        let result = call.promise.await;
                        assert_eq!(result.is_err(), fail);
                        if let Ok(response) = result {
                            assert_eq!(
                                value(Some(response.get().unwrap().get_cap().unwrap())).await,
                                73
                            );
                        }
                        drop(cap);
                        drop(call.pipeline);
                        drop(client);
                        drop(membrane);
                        settle().await;
                        drop(drivers);
                        settle().await;
                        assert_eq!(live.get(), 0);
                    }
                }
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn init_replaces_results_and_releases_old_capability() {
    struct Reset(Rc<Cell<u32>>);
    impl harness::Server for Reset {
        async fn pending(
            self: Rc<Self>,
            _: harness::PendingParams,
            mut r: harness::PendingResults,
        ) -> capnp::Result<()> {
            r.get_with_size_hint(Some(MessageSize {
                word_count: 0,
                cap_count: 0,
            }))
            .set_cap(Echo::client(1, &self.0));
            assert_eq!(self.0.get(), 1);
            r.init_with_size_hint(Some(MessageSize {
                word_count: u64::MAX,
                cap_count: u32::MAX,
            }));
            assert_eq!(self.0.get(), 0);
            r.init().set_cap(Echo::client(73, &self.0));
            Ok(())
        }
    }
    let live = Rc::new(Cell::new(0));
    let client: harness::Client = capnp_rpc::new_client(Reset(live.clone()));
    let response = client.pending_request().send().promise.await.unwrap();
    assert_eq!(
        value(Some(response.get().unwrap().get_cap().unwrap())).await,
        73
    );
    drop(response);
    assert_eq!(live.get(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn first_result_hint_reaches_transport_once_and_is_bounded_through_membranes() {
    use capnp_rpc::{
        membrane::{Direction, Membrane, Policy},
        RpcSystem,
    };
    struct Boundary;
    impl Policy for Boundary {
        fn call(
            &self,
            _: Direction,
            _: u64,
            _: u16,
            _: &capnp::capability::Client,
        ) -> capnp::Result<Option<capnp::capability::Client>> {
            Ok(None)
        }
    }
    struct Hinted(Option<MessageSize>);
    impl harness::Server for Hinted {
        async fn echo(
            self: Rc<Self>,
            _: harness::EchoParams,
            mut r: harness::EchoResults,
        ) -> capnp::Result<()> {
            r.get_with_size_hint(self.0).set_value(7);
            // A second hint must not replace the message or its populated data.
            assert_eq!(
                r.get_with_size_hint(Some(MessageSize {
                    word_count: 13,
                    cap_count: 0
                }))
                .get_value(),
                7
            );
            Ok(())
        }
    }
    tokio::task::LocalSet::new()
        .run_until(async {
            // Pinned RPC layout: 8 envelope words, 4 per capability plus list tag.
            for (hint, expected) in [
                (None, 0),
                (
                    Some(MessageSize {
                        word_count: 0,
                        cap_count: 0,
                    }),
                    8,
                ),
                (
                    Some(MessageSize {
                        word_count: 64,
                        cap_count: 1,
                    }),
                    77,
                ),
                (
                    Some(MessageSize {
                        word_count: u64::MAX,
                        cap_count: u32::MAX,
                    }),
                    (1 << 20) + 8,
                ),
            ] {
                for wrapped in [false, true] {
                    let client: harness::Client = capnp_rpc::new_client(Hinted(hint));
                    let boundary = Membrane::new(Rc::new(Boundary));
                    let client = if wrapped {
                        boundary.export(client)
                    } else {
                        client
                    };
                    let hub = Rc::new(RefCell::new(support::Hub::default()));
                    let host = RpcSystem::new(
                        Box::new(support::Hub::network(&hub, 1)),
                        Some(client.client),
                    );
                    let mut caller = RpcSystem::new(Box::new(support::Hub::network(&hub, 2)), None);
                    let client: harness::Client = caller.bootstrap(1);
                    let status = hub.borrow_mut().connect(1, 2).status();
                    let _drivers = Drivers(vec![
                        tokio::task::spawn_local(host),
                        tokio::task::spawn_local(caller),
                    ]);
                    client.as_client_hook().when_resolved().await.unwrap();
                    settle().await;
                    status.borrow_mut().allocation_hints.clear();
                    assert_eq!(
                        client
                            .echo_request()
                            .send()
                            .promise
                            .await
                            .unwrap()
                            .get()
                            .unwrap()
                            .get_value(),
                        7
                    );
                    assert_eq!(
                        status.borrow().allocation_hints,
                        [expected],
                        "hint={hint:?}, membrane={wrapped}"
                    );
                }
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn canceling_a_call_releases_unadopted_result_orphans() {
    struct Pending(Rc<Cell<u32>>);
    impl harness::Server for Pending {
        async fn pending(
            self: Rc<Self>,
            _: harness::PendingParams,
            mut r: harness::PendingResults,
        ) -> capnp::Result<()> {
            let (mut root, token) = r.get_orphanage(None)?;
            let orphan = new_orphan(&mut root, &token, 1, &self.0);
            futures::future::pending::<()>().await;
            drop(orphan);
            Ok(())
        }
    }
    tokio::task::LocalSet::new()
        .run_until(async {
            for wire in [false, true] {
                let live = Rc::new(Cell::new(0));
                let client: harness::Client = capnp_rpc::new_client(Pending(live.clone()));
                let mut drivers = Drivers(Vec::new());
                let client = if wire {
                    let hub = Rc::new(RefCell::new(support::Hub::default()));
                    let host = capnp_rpc::RpcSystem::new(
                        Box::new(support::Hub::network(&hub, 1)),
                        Some(client.client),
                    );
                    let mut caller =
                        capnp_rpc::RpcSystem::new(Box::new(support::Hub::network(&hub, 2)), None);
                    let client = caller.bootstrap(1);
                    drivers.0.push(tokio::task::spawn_local(host));
                    drivers.0.push(tokio::task::spawn_local(caller));
                    client
                } else {
                    client
                };
                let mut call = client.pending_request().send();
                for _ in 0..64 {
                    assert!((&mut call.promise).now_or_never().is_none());
                    tokio::task::yield_now().await;
                }
                assert_eq!(live.get(), 1);
                drop(call);
                settle().await;
                assert_eq!(live.get(), 0);
            }
        })
        .await;
}
