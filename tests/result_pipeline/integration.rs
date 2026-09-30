use super::super::*;
use capnp::{
    any_pointer, dynamic_capability as compiled,
    schema_loader::{dynamic as loaded, SchemaLoader},
};
use capnp_rpc::membrane::{Direction, Membrane, Policy};

#[tokio::test(flavor = "current_thread")]
async fn publication_resolves_existing_observers_without_finishing_parent() {
    for fail in [false, true] {
        let mut f = Fixture::new(false).await;
        let cap = f.pipeline.get_cap();
        let mut resolved = cap.as_client_hook().when_resolved();
        assert!((&mut resolved).now_or_never().is_none());
        f.call();
        f.publish().unwrap();
        if fail {
            f.finish(false);
        }
        f.resolve.take().unwrap().send(true).unwrap();
        f.drive().await;
        assert_eq!((f.done, f.failed), (1, 0));
        assert_eq!(f.parent_result, if fail { 2 } else { 0 });
        resolved
            .now_or_never()
            .expect("published capability must resolve independently")
            .unwrap();
    }
}

#[tokio::test(flavor = "current_thread")]
async fn snapshot_and_independent_publication_share_one_slot() {
    for snapshot_first in [false, true] {
        let mut f = Fixture::new(false).await;
        if snapshot_first {
            {
                let mut results = f.state.results.borrow_mut();
                let results = results.as_mut().unwrap();
                results.get().set_cap(f.target.clone());
                results.set_pipeline().unwrap();
            }
            assert!(f.publish().is_err());
        } else {
            f.publish().unwrap();
            assert!(f
                .state
                .results
                .borrow_mut()
                .as_mut()
                .unwrap()
                .set_pipeline()
                .is_err());
        }
        f.call();
        f.resolve.take().unwrap().send(true).unwrap();
        f.drive().await;
        assert_eq!((f.done, f.failed, f.parent_result), (1, 0, 0));
    }
}

struct Boundary {
    deny_echo: bool,
    calls: RefCell<Vec<(Direction, u16)>>,
}
impl Policy for Boundary {
    fn call(
        &self,
        direction: Direction,
        _: u64,
        method: u16,
        _: &capnp::capability::Client,
    ) -> capnp::Result<Option<capnp::capability::Client>> {
        self.calls.borrow_mut().push((direction, method));
        if self.deny_echo && method == 0 {
            Err(Error::failed("echo denied".into()))
        } else {
            Ok(None)
        }
    }
}
#[tokio::test(flavor = "current_thread")]
async fn independent_pipelines_obey_membrane_policy_and_revocation_in_both_directions() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for wire in [false, true] {
                for reverse in [false, true] {
                    for deny_echo in [false, true] {
                        let policy = Rc::new(Boundary {
                            deny_echo,
                            calls: RefCell::new(Vec::new()),
                        });
                        let membrane = Membrane::new(policy.clone());
                        let mut f = Fixture::with_wrapper(wire, |client| {
                            if reverse {
                                membrane.import(client)
                            } else {
                                membrane.export(client)
                            }
                        })
                        .await;
                        f.call();
                        f.publish().unwrap();
                        f.resolve.take().unwrap().send(true).unwrap();
                        f.drive().await;
                        assert_eq!((f.done, f.failed), if deny_echo { (0, 1) } else { (1, 0) });
                        assert_eq!(f.calls.get(), u32::from(!deny_echo));
                        let expected = if reverse {
                            Direction::Outbound
                        } else {
                            Direction::Inbound
                        };
                        assert_eq!(*policy.calls.borrow(), [(expected, 3), (expected, 0)]);
                        membrane.revoke(Error::failed("revoked".into()));
                        f.call();
                        f.drive().await;
                        assert_eq!((f.done, f.failed), if deny_echo { (0, 2) } else { (1, 1) });
                        assert_eq!(f.calls.get(), u32::from(!deny_echo));
                    }
                }
            }
        })
        .await;
}

struct CompiledServer {
    gate: RefCell<Option<oneshot::Receiver<()>>>,
    target: harness::Client,
}
impl compiled::Server for CompiledServer {
    fn get_schema(&self) -> capnp::schema::InterfaceSchema {
        harness::Client::schema()
    }
    fn allow_cancellation(&self) -> bool {
        true
    }
    fn call(
        self: Rc<Self>,
        method: capnp::schema::Method,
        mut context: compiled::CallContext,
    ) -> Promise<(), Error> {
        Promise::from_future(async move {
            assert_eq!(method.get_proto().get_name()?.to_str()?, "pending");
            let wrong = harness::Client::schema()
                .get_method_by_name("echo")?
                .get_result_type()
                .unwrap();
            let invalid = compiled::Pipeline::new(
                PipelineBuilder::<any_pointer::Owned>::new().build(),
                wrong,
            );
            assert!(context.set_pipeline_from(invalid).is_err());
            let mut builder = PipelineBuilder::<any_pointer::Owned>::new();
            builder
                .get()
                .init_as::<harness::pending_results::Builder>()
                .set_cap(self.target.clone());
            context.set_pipeline_from(compiled::Pipeline::new(
                builder.build(),
                method.get_result_type().unwrap(),
            ))?;
            let gate = self.gate.borrow_mut().take().unwrap();
            gate.await.map_err(|_| Error::failed("gate lost".into()))?;
            context
                .get_results()?
                .set_named("cap", compiled::Client::from(self.target.clone()).into())?;
            Ok(())
        })
    }
}
struct LoadedServer {
    gate: RefCell<Option<oneshot::Receiver<()>>>,
    schema: loaded::ServiceSchema,
    target: harness::Client,
}
impl loaded::Server for LoadedServer {
    fn allow_cancellation(&self) -> bool {
        true
    }
    fn call(self: Rc<Self>, mut context: loaded::CallContext) -> Promise<(), Error> {
        Promise::from_future(async move {
            let schema = self.schema.get()?;
            assert_eq!(
                context.method()?.get_proto().get_name()?.to_str()?,
                "pending"
            );
            let wrong = schema.method("echo")?.results()?;
            assert!(context
                .set_pipeline_from(loaded::Pipeline::new(
                    PipelineBuilder::<any_pointer::Owned>::new().build(),
                    wrong
                )?)
                .is_err());
            let mut builder = PipelineBuilder::<any_pointer::Owned>::new();
            builder
                .get()
                .init_as::<harness::pending_results::Builder>()
                .set_cap(self.target.clone());
            context.set_pipeline_from(loaded::Pipeline::new(
                builder.build(),
                schema.method("pending")?.results()?,
            )?)?;
            let gate = self.gate.borrow_mut().take().unwrap();
            gate.await.map_err(|_| Error::failed("gate lost".into()))?;
            context.get_results()?.set_named(
                "cap",
                loaded::Value::Capability(self.schema.reflect(self.target.clone())?),
            )?;
            Ok(())
        })
    }
}
#[tokio::test(flavor = "current_thread")]
async fn compiled_and_loaded_dynamic_servers_validate_and_publish_independent_pipelines() {
    for loaded in [false, true] {
        let (gate, rx) = oneshot::channel();
        let calls = Rc::new(Cell::new(0));
        let target = capnp_rpc::new_client(Echo(73, calls.clone()));
        let client: harness::Client = if loaded {
            let mut loader = SchemaLoader::default();
            loader
                .load_compiled_type_and_dependencies::<harness::Owned>()
                .unwrap();
            let schema = loaded::ServiceSchema::new(
                Rc::new(loader),
                harness::Client::schema().get_proto().get_id(),
            )
            .unwrap();
            capnp_rpc::new_loaded_client(
                LoadedServer {
                    gate: RefCell::new(Some(rx)),
                    schema: schema.clone(),
                    target,
                },
                schema,
            )
            .cast_to()
        } else {
            capnp_rpc::new_dynamic_client(CompiledServer {
                gate: RefCell::new(Some(rx)),
                target,
            })
            .as_client()
            .unwrap()
            .clone()
            .cast_to()
        };
        let mut parent = client.pending_request().send();
        assert!((&mut parent.promise).now_or_never().is_none());
        assert_eq!(
            parent
                .pipeline
                .get_cap()
                .echo_request()
                .send()
                .promise
                .await
                .unwrap()
                .get()
                .unwrap()
                .get_value(),
            73
        );
        assert!((&mut parent.promise).now_or_never().is_none());
        gate.send(()).unwrap();
        let response = parent.promise.await.unwrap();
        assert_eq!(
            response
                .get()
                .unwrap()
                .get_cap()
                .unwrap()
                .echo_request()
                .send()
                .promise
                .await
                .unwrap()
                .get()
                .unwrap()
                .get_value(),
            73
        );
        assert_eq!(calls.get(), 2);
    }
}

struct PublishThenFail(harness::Client);
impl harness::Server for PublishThenFail {
    async fn pending(
        self: Rc<Self>,
        _: harness::PendingParams,
        mut results: harness::PendingResults,
    ) -> capnp::Result<()> {
        let mut builder = PipelineBuilder::<harness::pending_results::Owned>::new();
        builder.get().set_cap(self.0.clone());
        results.set_pipeline_from(builder.build())?;
        Err(Error::failed(
            "published and failed during the same poll".into(),
        ))
    }
}
#[tokio::test(flavor = "current_thread")]
async fn publication_wins_when_driving_parent_publishes_and_fails_in_one_poll() {
    for observe_first in [false, true] {
        let (tx, rx) = oneshot::channel();
        let target = capnp_rpc::new_future_client(async move {
            rx.await.map_err(|_| Error::failed("target lost".into()))
        });
        let client: harness::Client = capnp_rpc::new_client(PublishThenFail(target));
        let parent = client.pending_request().send();
        let cap = parent.pipeline.get_cap();
        let mut child = cap.echo_request().send().promise;
        let mut observed = cap.as_client_hook().when_resolved();
        // Neither operation is ready, but polling either one drives the parent
        // through both publication and failure. Forwarding must win that race.
        if observe_first {
            assert!((&mut observed).now_or_never().is_none());
        }
        assert!((&mut child).now_or_never().is_none());
        assert!((&mut observed).now_or_never().is_none());
        let calls = Rc::new(Cell::new(0));
        assert!(tx
            .send(capnp_rpc::new_client(Echo(73, calls.clone())))
            .is_ok());
        assert_eq!(child.await.unwrap().get().unwrap().get_value(), 73);
        observed.await.unwrap();
        assert!(parent.promise.await.is_err());
        assert_eq!(calls.get(), 1);
    }
}

struct Relay(RefCell<Option<harness::Client>>, Rc<Cell<bool>>);
impl harness::Server for Relay {
    async fn pending(
        self: Rc<Self>,
        _: harness::PendingParams,
        mut results: harness::PendingResults,
    ) -> capnp::Result<()> {
        let downstream = self.0.borrow().as_ref().unwrap().pending_request().send();
        results.set_pipeline_from(downstream.pipeline)?;
        let response = downstream.promise.await?;
        results.set(response.get()?)?;
        self.1.set(true);
        Ok(())
    }
}
#[tokio::test(flavor = "current_thread")]
async fn returned_pipeline_can_be_forwarded_across_three_vats_before_final_response() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (gate, rx) = oneshot::channel();
            let state = Rc::new(State {
                results: RefCell::new(None),
                gate: RefCell::new(Some(rx)),
                running: Cell::new(false),
            });
            let host: harness::Client = capnp_rpc::new_client(Controlled(state.clone()));
            let hub = Rc::new(RefCell::new(support::Hub::default()));
            let host = RpcSystem::new(Box::new(support::Hub::network(&hub, 1)), Some(host.client));
            let completed = Rc::new(Cell::new(false));
            let relay = Rc::new(Relay(RefCell::new(None), completed.clone()));
            let relay_cap: harness::Client = capnp_rpc::new_client_from_rc(relay.clone());
            let mut middle = RpcSystem::new(
                Box::new(support::Hub::network(&hub, 2)),
                Some(relay_cap.client),
            );
            *relay.0.borrow_mut() = Some(middle.bootstrap(1));
            let mut caller = RpcSystem::new(Box::new(support::Hub::network(&hub, 3)), None);
            let client: harness::Client = caller.bootstrap(2);
            let _drivers = Drivers(vec![
                tokio::task::spawn_local(host),
                tokio::task::spawn_local(middle),
                tokio::task::spawn_local(caller),
            ]);
            let mut parent = client.pending_request().send();
            for _ in 0..64 {
                assert!((&mut parent.promise).now_or_never().is_none());
                tokio::task::yield_now().await;
            }
            assert!(state.results.borrow().is_some());
            let calls = Rc::new(Cell::new(0));
            let target: harness::Client = capnp_rpc::new_client(Echo(73, calls.clone()));
            let mut builder = PipelineBuilder::<harness::pending_results::Owned>::new();
            builder.get().set_cap(target.clone());
            state
                .results
                .borrow_mut()
                .as_mut()
                .unwrap()
                .set_pipeline_from(builder.build())
                .unwrap();
            let response = tokio::time::timeout(
                std::time::Duration::from_secs(3),
                parent.pipeline.get_cap().echo_request().send().promise,
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(response.get().unwrap().get_value(), 73);
            assert!(!completed.get());
            assert!((&mut parent.promise).now_or_never().is_none());
            state
                .results
                .borrow_mut()
                .take()
                .unwrap()
                .get()
                .set_cap(target);
            gate.send(true).unwrap();
            let response = parent.promise.await.unwrap();
            assert!(completed.get());
            assert_eq!(
                response
                    .get()
                    .unwrap()
                    .get_cap()
                    .unwrap()
                    .echo_request()
                    .send()
                    .promise
                    .await
                    .unwrap()
                    .get()
                    .unwrap()
                    .get_value(),
                73
            );
            assert_eq!(calls.get(), 2);
        })
        .await;
}
