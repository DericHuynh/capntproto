use super::*;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

type Owner = Route<'static, u8, Rc<dyn ClientHook>, 8>;
type Graph<'id, 's> = clients::Graph<'id, 's, Owner, u8>;
fn with_graph(maximum: usize, f: impl for<'id> FnOnce(&mut Graph<'id, '_>)) {
    clients::with_growing_graph(0, maximum, f).unwrap();
}
#[derive(Clone)]
struct Service;
impl capability::Server for Service {
    fn as_ptr(&self) -> usize {
        1
    }
    fn dispatch_call(
        self,
        _: u64,
        _: u16,
        _: capability::Params<any_pointer::Owned>,
        _: capability::Results<any_pointer::Owned>,
    ) -> capability::DispatchCallResult {
        capability::DispatchCallResult {
            promise: Promise::ok(()),
            is_streaming: false,
            allow_cancellation: true,
        }
    }
}
fn settled() -> capability::Client {
    capability::Client::new(Box::new(crate::local::Client::new(Service)))
}
fn pending() -> (
    shared::Sender<capnp::Result<capability::Client>>,
    capability::Client,
) {
    let (sender, receiver) = shared::channel();
    (
        sender,
        crate::new_future_client(async move { receiver.await.map_err(crate::canceled_to_error)? }),
    )
}
fn poll_tied<'id>(
    foreign: &mut ForeignPromises<'id>,
    hooks: &mut ClientHooks<'id>,
    graph: &mut Graph<'id, '_>,
) -> Poll<capnp::Result<Progress>> {
    foreign.poll(
        hooks,
        graph,
        &mut Context::from_waker(Waker::noop()),
        NonZeroUsize::new(1).unwrap(),
    )
}
fn drive<'id>(
    foreign: &mut ForeignPromises<'id>,
    hooks: &mut ClientHooks<'id>,
    graph: &mut Graph<'id, '_>,
) {
    for _ in 0..32 {
        if let Poll::Ready(result) = poll_tied(foreign, hooks, graph) {
            result.unwrap();
        }
        if foreign.is_empty() {
            return;
        }
    }
    panic!("promise failed to settle");
}
fn ready<T>(promise: &mut Promise<T, Error>) -> capnp::Result<T> {
    match Pin::new(promise).poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(v) => v,
        Poll::Pending => panic!("pending"),
    }
}
fn drain(graph: &mut Graph<'_, '_>) {
    while graph.take_retired().is_some() {}
}

#[test]
fn chained_foreign_promise_stays_advertised_and_keeps_original_proxy() {
    with_graph(16, |graph| {
        let mut foreign = ForeignPromises::new(2);
        let mut hooks = ClientHooks::new(8, 8);
        let (first, source) = pending();
        let (second, middle) = pending();
        let client = foreign.import(graph, &*source.hook).unwrap();
        let original = match graph.inspect(&client).unwrap() {
            Destination::Endpoint { key, .. } => key,
            _ => panic!(),
        };
        let (snapshot, unresolved) = graph.snapshot_for_export(&client).unwrap();
        assert!(unresolved, "callable does not mean settled");
        assert_eq!(snapshot.key(), client.key());
        let exported = hooks
            .export_client_with_metadata(graph, &client, metadata)
            .unwrap();
        let mut resolved = exported.hook.when_resolved();
        assert!(Pin::new(&mut resolved)
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending());
        first.send(Ok(middle)).ok().unwrap();
        for _ in 0..8 {
            let _ = poll_tied(&mut foreign, &mut hooks, graph);
        }
        assert!(!graph.is_settled(&client).unwrap());
        second.send(Ok(settled())).ok().unwrap();
        drive(&mut foreign, &mut hooks, graph);
        ready(&mut resolved).unwrap();
        assert!(graph.is_settled(&client).unwrap());
        assert!(
            matches!(graph.inspect(&client).unwrap(), Destination::Endpoint { key, .. } if key == original)
        );
        let (settled_snapshot, unresolved) = graph.snapshot_for_export(&client).unwrap();
        assert!(!unresolved);
        assert_eq!(settled_snapshot.key(), original);
        drop((source, exported, resolved));
        graph.release(client);
        graph.release(snapshot);
        graph.release(settled_snapshot);
        hooks.close(graph, Error::disconnected("done".into()));
        foreign.terminate();
        drain(graph);
        assert!(graph.is_empty());
    });
}
#[test]
fn failed_resolution_retries_capacity_once_and_retains_full_diagnostics() {
    with_graph(3, |graph| {
        let mut foreign = ForeignPromises::new(1);
        let mut hooks = ClientHooks::new(8, 8);
        let (sender, source) = pending();
        let client = foreign.import(graph, &*source.hook).unwrap();
        let exported = hooks
            .export_client_with_metadata(graph, &client, metadata)
            .unwrap();
        let mut resolved = exported.hook.when_resolved();
        let blocker = graph
            .endpoint(Route::local(Rc::from(settled().hook)), Location::Local)
            .unwrap_or_else(|e| panic!("{}", e.error));
        let mut error = Error::overloaded("foreign resolution detail".into());
        error.set_detail(71, vec![3, 1, 4]);
        sender.send(Err(error.clone())).ok().unwrap();
        // The queued hook first resolves to a broken hook; observing that
        // hook's next resolution yields the original exception.
        for _ in 0..8 {
            if matches!(
                poll_tied(&mut foreign, &mut hooks, graph),
                Poll::Ready(Err(_))
            ) {
                break;
            }
        }
        for _ in 0..3 {
            let Poll::Ready(Err(error)) = poll_tied(&mut foreign, &mut hooks, graph) else {
                panic!()
            };
            assert_eq!(error.kind, capnp::ErrorKind::Overloaded);
            assert!(!graph.is_settled(&client).unwrap());
        }
        graph.release(blocker);
        drain(graph);
        drive(&mut foreign, &mut hooks, graph);
        let failure = ready(&mut resolved).unwrap_err();
        assert_eq!(failure.extra, error.extra);
        assert_eq!(failure.detail(71), Some(&[3, 1, 4][..]));
        let later = hooks
            .export_client_with_metadata(graph, &client, metadata)
            .unwrap();
        assert_eq!(
            ready(&mut later.hook.when_resolved()).unwrap_err().extra,
            error.extra
        );
        match graph.inspect(&client).unwrap() {
            Destination::Endpoint {
                location: Location::Broken,
                owner,
                ..
            } => {
                use capntproto_rpc_wire::clients::EndpointOwner;
                assert_eq!(owner.exception(), Some(error.kind));
                assert_eq!(
                    metadata(owner).failure.unwrap().detail(71),
                    error.detail(71)
                );
            }
            _ => panic!(),
        }
        drop((source, exported, later, resolved));
        graph.release(client);
        hooks.close(graph, Error::disconnected("done".into()));
        foreign.terminate();
        drain(graph);
        assert!(graph.is_empty());
    });
}
#[test]
fn abandoned_observer_reclaims_capacity_without_retaining_graph_authority() {
    with_graph(2, |graph| {
        let mut foreign = ForeignPromises::new(1);
        let mut hooks = ClientHooks::new(2, 2);
        let (sender, source) = pending();
        let client = foreign.import(graph, &*source.hook).unwrap();
        assert_eq!(
            foreign.import(graph, &*source.hook).unwrap_err().kind,
            capnp::ErrorKind::Overloaded
        );
        graph.release(client);
        drain(graph);
        assert!(graph.is_empty(), "observer is a weak key");
        assert!(matches!(
            poll_tied(&mut foreign, &mut hooks, graph),
            Poll::Ready(Ok(Progress::Abandoned))
        ));
        let client = foreign.import(graph, &*source.hook).unwrap();
        sender.send(Ok(settled())).ok().unwrap();
        drive(&mut foreign, &mut hooks, graph);
        assert!(
            graph.is_settled(&client).unwrap(),
            "success needs no extra graph node"
        );
        graph.release(client);
        drop(source);
        foreign.terminate();
        hooks.close(graph, Error::disconnected("done".into()));
        drain(graph);
        assert!(graph.is_empty());
    });
}
struct WakeCount(AtomicUsize);
impl std::task::Wake for WakeCount {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
#[test]
fn budget_yields_fairly_and_idle_promises_sleep_until_notified() {
    with_graph(8, |graph| {
        let mut foreign = ForeignPromises::new(3);
        let mut hooks = ClientHooks::new(2, 2);
        let (sender, pending) = pending();
        let a = foreign.import(graph, &*pending.hook).unwrap();
        let ready_client: capability::Client = crate::new_future_client(async { Ok(settled()) });
        let b = foreign.import(graph, &*ready_client.hook).unwrap();
        let count = Arc::new(WakeCount(AtomicUsize::new(0)));
        let waker = Waker::from(count.clone());
        let mut cx = Context::from_waker(&waker);
        for _ in 0..16 {
            if let Poll::Ready(result) =
                foreign.poll(&mut hooks, graph, &mut cx, NonZeroUsize::new(1).unwrap())
            {
                result.unwrap();
            }
            if graph.is_settled(&b).unwrap() {
                break;
            }
        }
        assert!(graph.is_settled(&b).unwrap());
        assert!(!graph.is_settled(&a).unwrap());
        // Complete an idle scan; a whole scan with sufficient budget must sleep.
        let _ = foreign.poll(&mut hooks, graph, &mut cx, NonZeroUsize::new(8).unwrap());
        count.0.store(0, Ordering::SeqCst);
        assert!(foreign
            .poll(&mut hooks, graph, &mut cx, NonZeroUsize::new(8).unwrap())
            .is_pending());
        assert_eq!(count.0.load(Ordering::SeqCst), 0);
        sender.send(Ok(settled())).ok().unwrap();
        assert!(count.0.load(Ordering::SeqCst) > 0);
        drive(&mut foreign, &mut hooks, graph);
        graph.release(a);
        graph.release(b);
        drop((pending, ready_client));
        foreign.terminate();
        hooks.close(graph, Error::disconnected("done".into()));
        drain(graph);
        assert!(graph.is_empty());
    });
}
#[test]
fn opaque_hook_identity_does_not_deduplicate_or_unwrap_policy() {
    // The wrapper deliberately exposes the inner identity but hides resolution.
    // It must stay opaque, even when the original is a bridge-owned hook.
    with_graph(8, |graph| {
        let mut foreign = ForeignPromises::new(0);
        let mut hooks = ClientHooks::new(2, 2);
        let source = settled();
        let route = foreign.import(graph, &*source.hook).unwrap();
        let exported = hooks.export_client(graph, &route).unwrap();
        let wrapped = crate::scoped::tests::Opaque(exported.hook.add_ref());
        assert!(hooks.try_retain(graph, &wrapped).unwrap().is_none());
        let imported = foreign.import(graph, &wrapped).unwrap();
        assert_ne!(route.key(), imported.key());
        assert!(foreign.is_empty());
        assert!(matches!(
            graph.inspect(&imported).unwrap(),
            Destination::Endpoint {
                location: Location::Local,
                ..
            }
        ));
        graph.release(imported);
        drain(graph);
        drop((wrapped, exported, source));
        graph.release(route);
        hooks.close(graph, Error::disconnected("done".into()));
        foreign.terminate();
        drain(graph);
        assert!(graph.is_empty());
    });
}
