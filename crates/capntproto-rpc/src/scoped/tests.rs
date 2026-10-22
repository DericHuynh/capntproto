use super::*;
use capnp::capability::FromClientHook;
use capntproto_rpc_engine::clients::{self, dispatch, Destination, Location};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

type Remote = capability::RemotePromise<any_pointer::Owned>;

#[test]
fn metadata_subscriptions_preserve_late_changes_and_dynamic_projection() {
    clients::with_growing_graph::<Rc<MetadataSource>, u8, _>(0, 8, |graph| {
        let source = Rc::new(MetadataSource::default());
        let route = graph
            .endpoint(source.clone(), Location::Local)
            .unwrap_or_else(|_| panic!("endpoint capacity"));
        let mut hooks = ClientHooks::new(8, 1);
        let client = hooks.export_snapshot(graph, &route).unwrap();
        let reads = Cell::new(0);
        for _ in 0..100 {
            hooks.synchronize(
                graph,
                |owner| owner.snapshot(),
                |owner| {
                    reads.set(reads.get() + 1);
                    Some(owner.clone())
                },
                true,
                true,
            );
        }
        assert_eq!(reads.get(), 1, "stable imports project metadata once");
        assert!(
            hooks.refresh.is_empty(),
            "subscribed endpoints leave the refresh set"
        );
        assert!(client.hook.get_failure().is_none());
        source
            .update(|metadata| metadata.failure = Some(Error::overloaded("late metadata".into())));
        assert_eq!(
            client.hook.get_failure().unwrap().extra,
            "late metadata",
            "source updates publish without a collection walk"
        );
        hooks.synchronize(
            graph,
            |_| panic!("subscribed owner needs no graph projection"),
            |_| unreachable!(),
            true,
            true,
        );
        assert_eq!(client.hook.get_failure().unwrap().extra, "late metadata");
        hooks.collect_with_metadata(graph, |_| EndpointMetadata {
            failure: Some(Error::failed("dynamic override".into())),
            ..Default::default()
        });
        assert_eq!(client.hook.get_failure().unwrap().extra, "dynamic override");
        source.update(|metadata| metadata.failure = Some(Error::failed("late metadata".into())));
        assert_eq!(
            client.hook.get_failure().unwrap().extra,
            "dynamic override",
            "old source lease was disarmed"
        );
        hooks.synchronize(
            graph,
            |owner| owner.snapshot(),
            |owner| Some(owner.clone()),
            true,
            true,
        );
        assert_eq!(client.hook.get_failure().unwrap().extra, "late metadata");
        source.update(|metadata| metadata.failure = Some(Error::failed("later revision".into())));
        hooks.synchronize(graph, |_| unreachable!(), |_| unreachable!(), true, true);
        assert_eq!(client.hook.get_failure().unwrap().extra, "later revision");
        #[cfg(all(unix, not(miri)))]
        {
            let (resource, _peer) = std::os::unix::net::UnixStream::pair().unwrap();
            let fd = Rc::new(std::os::fd::OwnedFd::from(resource));
            assert!(client.hook.get_fd().is_none());
            source.update(|metadata| metadata.fd = Some(fd.clone()));
            hooks.synchronize(graph, |_| unreachable!(), |_| unreachable!(), true, true);
            assert!(Rc::ptr_eq(&fd, &client.hook.get_fd().unwrap()));
        }
        drop(client);
        hooks.collect(graph);
        graph.release(route);
        assert!(graph.take_retired().is_some());
    })
    .unwrap();
}

#[test]
fn release_journal_tracks_final_owners_without_growing_on_drop() {
    clients::with_growing_graph::<usize, u8, _>(0, 64, |graph| {
        let mut hooks = ClientHooks::new(64, 1);
        let mut owners = Vec::new();
        for value in 0..64 {
            let route = graph.endpoint(value, Location::Local).unwrap();
            let client = hooks.export_snapshot(graph, &route).unwrap();
            let alias = hooks.export_snapshot(graph, &route).unwrap();
            graph.release(route);
            drop(alias);
            owners.push(client);
        }
        assert!(hooks.inbox.released.borrow().is_empty());
        let capacity = hooks.inbox.released.borrow().capacity();
        // No collection between drops: every published slot needs a record.
        while let Some(owner) = owners.pop() {
            drop(owner);
            assert_eq!(hooks.inbox.released.borrow().capacity(), capacity);
        }
        assert!(graph.take_retired().is_none());
        hooks.reclaim_released(graph);
        for value in (0..64).rev() {
            assert_eq!(graph.take_retired(), Some(value));
        }
        assert!(graph.is_empty());
        // Reuse every index after consuming its old notification. A later
        // notification must only retire the new endpoint that owns that slot.
        for value in 64..128 {
            let route = graph.endpoint(value, Location::Local).unwrap();
            owners.push(hooks.export_snapshot(graph, &route).unwrap());
            graph.release(route);
        }
        let last = owners.pop().unwrap();
        drop(last);
        hooks.reclaim_released(graph);
        assert_eq!(graph.take_retired(), Some(127));
        assert!(graph.take_retired().is_none());
        for owner in owners {
            drop(owner);
        }
        hooks.reclaim_released(graph);
        for value in 64..127 {
            assert_eq!(graph.take_retired(), Some(value));
        }
        assert!(graph.is_empty());
    })
    .unwrap();
}

#[test]
fn release_processing_does_not_publish_promise_resolution() {
    clients::with_growing_graph::<(), u8, _>(0, 3, |graph| {
        let route = graph.promise(&mut None).unwrap();
        let mut hooks = ClientHooks::new(1, 1);
        let client = hooks.export_client(graph, &route).unwrap();
        let mut resolved = client.hook.when_resolved();
        let mut broken = Some(graph.endpoint((), Location::Broken).unwrap());
        graph
            .resolve(route.key(), &mut broken, clients::Source::Direct)
            .unwrap();
        hooks.reclaim_released(graph);
        assert!(Pin::new(&mut resolved)
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending());
        hooks.collect_with_metadata(graph, |_| EndpointMetadata {
            failure: Some(Error::overloaded(
                "specific failure before notification".into(),
            )),
            ..EndpointMetadata::default()
        });
        let failure = ready(&mut resolved).unwrap_err();
        assert_eq!(failure.kind, capnp::ErrorKind::Overloaded);
        assert_eq!(failure.extra, "specific failure before notification");
        assert_eq!(hooks.unsettled, 0);
        graph.release(route);
        drop((resolved, client));
        hooks.reclaim_released(graph);
        assert_eq!(graph.take_retired(), Some(()));
        assert!(graph.is_empty());
    })
    .unwrap();
}

#[test]
fn metadata_callback_can_release_another_endpoint_during_collection() {
    clients::with_growing_graph::<u32, u8, _>(0, 2, |graph| {
        let mut hooks = ClientHooks::new(2, 1);
        let first = graph.endpoint(1, Location::Local).unwrap();
        let second = graph.endpoint(2, Location::Local).unwrap();
        let client = hooks.export_snapshot(graph, &first).unwrap();
        let mut other = Some(hooks.export_snapshot(graph, &second).unwrap());
        graph.release(first);
        graph.release(second);
        hooks.collect_with_metadata(graph, |owner| {
            assert_eq!(*owner, 1);
            drop(other.take());
            EndpointMetadata::default()
        });
        assert_eq!(graph.take_retired(), Some(2));
        assert!(graph.take_retired().is_none());
        drop(client);
        hooks.reclaim_released(graph);
        assert_eq!(graph.take_retired(), Some(1));
        assert!(graph.is_empty());
    })
    .unwrap();
}

fn request(client: &capability::Client, method: u16) -> Remote {
    client
        .new_call::<any_pointer::Owned, any_pointer::Owned>(47, method, None)
        .send()
}
fn next(hooks: &mut ClientHooks<'_>) -> Call {
    let Poll::Ready(Some(call)) = hooks.poll_next(&mut Context::from_waker(Waker::noop())) else {
        panic!("expected queued call")
    };
    call
}
fn ready<F: Future + Unpin>(future: &mut F) -> F::Output {
    let Poll::Ready(result) = Pin::new(future).poll(&mut Context::from_waker(Waker::noop())) else {
        panic!("expected completed future")
    };
    result
}
fn error<T>(result: capnp::Result<T>) -> Error {
    match result {
        Err(error) => error,
        Ok(_) => panic!("expected failure"),
    }
}
struct Wakes(AtomicUsize);
impl std::task::Wake for Wakes {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

#[test]
fn only_an_unfinished_reply_drop_wakes_the_inbox() {
    for state in 0..4 {
        let mut hooks = ClientHooks::new(1, 1);
        let (sender, receiver) = shared::channel();
        let mut sender = Some(sender);
        let mut reply = Reply {
            receiver: Some(receiver),
            shutdown: hooks.inbox.shutdown.clone(),
            inbox: Rc::downgrade(&hooks.inbox),
        };
        if state == 1 || state == 2 {
            let result = if state == 1 {
                Ok(())
            } else {
                Err(Error::failed("rejected".into()))
            };
            sender.take().unwrap().send(result).unwrap();
        } else if state == 3 {
            drop(sender.take());
        }
        let result = Pin::new(&mut reply).poll(&mut Context::from_waker(Waker::noop()));
        assert_eq!(result.is_pending(), state == 0);
        if let Poll::Ready(result) = result {
            assert_eq!(result.is_ok(), state == 1);
        }
        let wakes = Arc::new(Wakes(AtomicUsize::new(0)));
        let waker = Waker::from(wakes.clone());
        assert!(hooks
            .poll_next(&mut Context::from_waker(&waker))
            .is_pending());
        drop(reply);
        assert_eq!(wakes.0.load(Ordering::Relaxed), usize::from(state == 0));
        if let Some(sender) = sender {
            assert!(sender.is_closed());
        }
    }
}

#[test]
fn generated_client_uses_scoped_engine_dispatch_and_borrowed_service_owner() {
    use crate::persistent_capnp::persistent;
    type Text = capnp::text::Owned;
    let service = String::from("caller-owned service");
    let mut clients = [const { clients::Slot::EMPTY }; 2];
    let mut tasks = [const { dispatch::TaskSlot::EMPTY }; 2];
    dispatch::with_runtime(
        dispatch::Storage {
            clients: &mut clients,
            tasks: &mut tasks,
        },
        |runtime| {
            let route = runtime
                .endpoint(service.as_str(), Location::<u8>::Local)
                .unwrap();
            let mut hooks = ClientHooks::new(1, 2);
            let client = hooks.export_snapshot(runtime, &route).unwrap();
            let typed: persistent::Client<Text, Text> = FromClientHook::new(client.hook);
            let mut request = typed.save_request();
            request.get().set_seal_for("owner").unwrap();
            let mut result = request.send();
            assert!(matches!(
                hooks.poll_dispatch(runtime, &mut Context::from_waker(Waker::noop())),
                Poll::Ready(Ok(Some(_)))
            ));
            let queued = runtime.next_ready().unwrap().unwrap();
            assert!(matches!(
                queued.destination(),
                Destination::Endpoint {
                    owner: &"caller-owned service",
                    ..
                }
            ));
            let mut running = queued.accept();
            let call = running.owner_mut();
            assert_eq!(call.method_id(), 0);
            let params = call
                .params()
                .unwrap()
                .get_as::<persistent::save_params::Reader<'_, Text, Text>>()
                .unwrap();
            assert_eq!(params.get_seal_for().unwrap(), "owner");
            call.results()
                .get()
                .unwrap()
                .get_as::<persistent::save_results::Builder<'_, Text, Text>>()
                .unwrap()
                .set_sturdy_ref("saved")
                .unwrap();
            runtime.complete(running).complete(Ok(()));
            let response = ready(&mut result.promise).unwrap();
            assert_eq!(response.get().unwrap().get_sturdy_ref().unwrap(), "saved");
            drop((response, result, typed));
            hooks.collect(runtime);
            runtime.release(route);
            hooks.close(runtime, Error::disconnected("done".into()));
        },
    )
    .unwrap();
}

#[test]
fn snapshots_deduplicate_reclaim_and_bound_pending_promises() {
    clients::with_growing_graph::<u32, u8, _>(0, 4, |graph| {
        let route = graph.endpoint(7, Location::Local).unwrap();
        let other = graph.endpoint(8, Location::Local).unwrap();
        let pending = graph.promise(&mut None).unwrap();
        let mut hooks = ClientHooks::new(1, 1);
        let a = hooks.export_snapshot(graph, &route).unwrap();
        let b = hooks.export_snapshot(graph, &route).unwrap();
        assert_eq!(a.hook.get_ptr(), b.hook.get_ptr());
        assert_eq!(
            error(hooks.export_snapshot(graph, &pending)).kind,
            capnp::ErrorKind::Overloaded
        );
        assert_eq!(
            error(hooks.export_snapshot(graph, &other)).kind,
            capnp::ErrorKind::Overloaded
        );
        graph.release(route);
        drop(a);
        hooks.collect(graph);
        assert!(graph.take_retired().is_none());
        drop(b);
        hooks.collect(graph);
        assert_eq!(graph.take_retired(), Some(7));
        let c = hooks.export_snapshot(graph, &other).unwrap();
        drop(c);
        graph.release(other);
        graph.release(pending);
        hooks.close(graph, Error::disconnected("done".into()));
        assert_eq!(graph.take_retired(), Some(8));
        assert!(graph.is_empty());
    })
    .unwrap();
}

#[test]
fn pending_local_chains_wait_for_settlement_and_dispatch_in_order() {
    let mut nodes = [const { clients::Slot::EMPTY }; 4];
    let mut tasks = [const { dispatch::TaskSlot::EMPTY }; 3];
    dispatch::with_runtime(
        dispatch::Storage {
            clients: &mut nodes,
            tasks: &mut tasks,
        },
        |runtime| {
            let first = runtime.promise(&mut None).unwrap();
            let second = runtime.promise(&mut None).unwrap();
            let destination = runtime.endpoint(77u32, Location::<u8>::Local).unwrap();
            let mut hooks = ClientHooks::new(2, 3);
            let owned = hooks.export_client(runtime, &first).unwrap();
            let mut waiter = owned.hook.when_more_resolved().unwrap();
            let mut ready_waiter = owned.hook.when_resolved();
            let mut cx = Context::from_waker(Waker::noop());
            assert!(Pin::new(&mut waiter).poll(&mut cx).is_pending());
            let mut a = request(&owned, 1);
            let mut b = request(&owned, 2);
            for _ in 0..2 {
                assert!(matches!(
                    hooks.poll_dispatch(runtime, &mut cx),
                    Poll::Ready(Ok(Some(_)))
                ));
            }
            assert!(runtime.next_ready().unwrap().is_none());
            let mut replacement = Some(runtime.retain(&second).unwrap());
            assert!(runtime
                .resolve(first.key(), &mut replacement, clients::Source::Direct)
                .unwrap()
                .is_none());
            hooks.collect(runtime);
            assert!(Pin::new(&mut ready_waiter).poll(&mut cx).is_pending());
            let mut replacement = Some(runtime.retain(&destination).unwrap());
            assert!(runtime
                .resolve(second.key(), &mut replacement, clients::Source::Direct)
                .unwrap()
                .is_none());
            hooks.collect(runtime);
            let resolved = ready(&mut waiter).unwrap();
            assert!(resolved.when_more_resolved().is_none());
            ready(&mut ready_waiter).unwrap();
            let mut c = request(&owned, 3);
            assert!(matches!(
                hooks.poll_dispatch(runtime, &mut cx),
                Poll::Ready(Ok(Some(_)))
            ));
            for method in 1..=3 {
                let running = runtime.next_ready().unwrap().unwrap().accept();
                assert_eq!(running.owner().method_id(), method);
                runtime.complete(running).complete(Ok(()));
            }
            ready(&mut a.promise).unwrap();
            ready(&mut b.promise).unwrap();
            ready(&mut c.promise).unwrap();
            hooks.close(runtime, Error::disconnected("done".into()));
            runtime.release(first);
            runtime.release(second);
            runtime.release(destination);
        },
    )
    .unwrap();
}

#[test]
fn live_import_promise_waits_for_reflected_barrier_but_snapshot_stays_remote() {
    clients::with_growing_graph::<u32, u8, _>(0, 5, |graph| {
        let remote = graph.endpoint(7, Location::Remote(1)).unwrap();
        let mut route = Some(graph.retain(&remote).unwrap());
        let promised = graph.promise(&mut route).unwrap();
        assert!(!graph.is_settled(&promised).unwrap());
        assert!(matches!(
            graph.inspect(&promised).unwrap(),
            Destination::Endpoint { owner: &7, .. }
        ));
        let mut hooks = ClientHooks::new(3, 2);
        let live = hooks.export_client(graph, &promised).unwrap();
        let frozen = hooks.export_snapshot(graph, &promised).unwrap();
        assert!(frozen.hook.when_more_resolved().is_none());
        let mut waiter = live.hook.when_resolved();
        let called = graph.prepare_call(&promised).unwrap().accept();
        graph.release(called);
        let local = graph.endpoint(8, Location::Local).unwrap();
        let mut replacement = Some(graph.retain(&local).unwrap());
        let barrier = graph
            .resolve(promised.key(), &mut replacement, clients::Source::Reflected)
            .unwrap()
            .unwrap();
        hooks.collect(graph);
        assert!(Pin::new(&mut waiter)
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending());
        graph.open(barrier);
        hooks.collect(graph);
        ready(&mut waiter).unwrap();
        let live_call = request(&live, 1);
        let frozen_call = request(&frozen, 2);
        let call = next(&mut hooks);
        assert!(matches!(
            graph.inspect(hooks.target(&call).unwrap()).unwrap(),
            Destination::Endpoint { owner: &8, .. }
        ));
        call.complete(Ok(()));
        let call = next(&mut hooks);
        assert!(matches!(
            graph.inspect(hooks.target(&call).unwrap()).unwrap(),
            Destination::Endpoint { owner: &7, .. }
        ));
        call.complete(Ok(()));
        drop((live_call, frozen_call));
        hooks.close(graph, Error::disconnected("done".into()));
        graph.release(promised);
        graph.release(local);
        graph.release(remote);
        while graph.take_retired().is_some() {}
        assert!(graph.is_empty());
    })
    .unwrap();
}

#[test]
fn unobserved_promise_routes_settle_before_the_first_resolution_waiter() {
    for broken in [false, true] {
        clients::with_growing_graph::<u32, u8, _>(0, 2, |graph| {
            let pending = graph.promise(&mut None).unwrap();
            let mut hooks = ClientHooks::new(1, 1);
            let client = hooks.export_client(graph, &pending).unwrap();
            let endpoint = hooks.entries[0]
                .as_ref()
                .unwrap()
                .endpoint
                .upgrade()
                .unwrap();
            assert!(endpoint.resolution.borrow().is_none());
            let location = if broken {
                Location::Broken
            } else {
                Location::Local
            };
            let mut replacement = Some(graph.endpoint(7, location).unwrap());
            assert!(graph
                .resolve(pending.key(), &mut replacement, clients::Source::Direct)
                .unwrap()
                .is_none());
            hooks.collect(graph);
            assert!(endpoint.settled.get());
            assert!(endpoint.resolution.borrow().is_none());
            assert_eq!(ready(&mut client.hook.when_resolved()).is_err(), broken);
            drop((endpoint, client));
            graph.release(pending);
            hooks.collect(graph);
            assert_eq!(graph.take_retired(), Some(7));
            assert!(graph.is_empty());
        })
        .unwrap();
    }
}

#[test]
fn resolution_observers_retain_routes_and_wake_after_detachment() {
    clients::with_growing_graph::<u32, u8, _>(0, 2, |graph| {
        let pending = graph.promise(&mut None).unwrap();
        let mut hooks = ClientHooks::new(1, 1);
        let client = hooks.export_client(graph, &pending).unwrap();
        let endpoint = hooks.entries[0].as_ref().unwrap().endpoint.clone();
        let mut polled = client.hook.when_resolved();
        let mut unpolled = client.hook.when_resolved();
        let observed = Rc::new(Cell::new(false));
        REENTER.with(|slot| {
            let endpoint = endpoint.clone();
            let observed = observed.clone();
            *slot.borrow_mut() = Some(Box::new(move || {
                let endpoint = endpoint.upgrade().unwrap();
                assert!(endpoint.settled.get());
                assert!(endpoint.resolution.try_borrow_mut().unwrap().is_none());
                ready(&mut Hook(endpoint, None).when_resolved()).unwrap();
                observed.set(true);
            }));
        });
        let waker = Waker::from(Arc::new(Reenter));
        assert!(Pin::new(&mut polled)
            .poll(&mut Context::from_waker(&waker))
            .is_pending());
        drop(client);
        hooks.collect(graph);
        assert!(endpoint.upgrade().is_some());
        let mut replacement = Some(graph.endpoint(7, Location::Local).unwrap());
        assert!(graph
            .resolve(pending.key(), &mut replacement, clients::Source::Direct)
            .unwrap()
            .is_none());
        hooks.collect(graph);
        assert!(observed.get());
        ready(&mut polled).unwrap();
        ready(&mut unpolled).unwrap();
        drop((polled, unpolled));
        assert!(endpoint.upgrade().is_none());
        graph.release(pending);
        hooks.collect(graph);
        assert_eq!(graph.take_retired(), Some(7));
        assert!(graph.is_empty());
    })
    .unwrap();
}

#[test]
fn resolution_observers_disconnect_when_the_bridge_is_dropped() {
    for poll_first in [false, true] {
        clients::with_growing_graph::<u32, u8, _>(0, 1, |graph| {
            let pending = graph.promise(&mut None).unwrap();
            let mut hooks = ClientHooks::new(1, 1);
            let client = hooks.export_client(graph, &pending).unwrap();
            let endpoint = hooks.entries[0].as_ref().unwrap().endpoint.clone();
            let mut waiter = client.hook.when_resolved();
            let wakes = Arc::new(Wakes(AtomicUsize::new(0)));
            let waker = Waker::from(wakes.clone());
            if poll_first {
                assert!(Pin::new(&mut waiter)
                    .poll(&mut Context::from_waker(&waker))
                    .is_pending());
            }
            drop(client);
            graph.release(pending);
            drop(hooks);
            assert_eq!(wakes.0.load(Ordering::Relaxed) > 0, poll_first);
            let failure = error(ready(&mut waiter));
            assert_eq!(failure.kind, capnp::ErrorKind::Disconnected);
            assert_eq!(failure.extra, "scoped ClientHook driver dropped");
            drop(waiter);
            assert!(endpoint.upgrade().is_none());
        })
        .unwrap();
    }
}

#[test]
fn unresolved_waiters_retain_the_graph_route_and_observe_shutdown() {
    clients::with_growing_graph::<u32, u8, _>(0, 2, |graph| {
        let pending = graph.promise(&mut None).unwrap();
        let mut hooks = ClientHooks::new(1, 1);
        let client = hooks.export_client(graph, &pending).unwrap();
        let mut waiter = client.hook.when_resolved();
        drop(client);
        graph.release(pending);
        hooks.collect(graph);
        assert!(!graph.is_empty());
        hooks.close(graph, Error::disconnected("test disconnect".into()));
        assert_eq!(error(ready(&mut waiter)).extra, "test disconnect");
        assert!(graph.is_empty());
    })
    .unwrap();
}

#[test]
fn broken_resolution_fails_waiters_and_rejects_later_calls() {
    clients::with_growing_graph::<u32, u8, _>(0, 2, |graph| {
        let pending = graph.promise(&mut None).unwrap();
        let mut hooks = ClientHooks::new(1, 1);
        let client = hooks.export_client(graph, &pending).unwrap();
        let mut waiter = client.hook.when_resolved();
        let mut replacement = Some(graph.endpoint(0, Location::Broken).unwrap());
        assert!(graph
            .resolve(pending.key(), &mut replacement, clients::Source::Direct)
            .unwrap()
            .is_none());
        hooks.collect(graph);
        assert!(ready(&mut waiter).is_err());
        assert!(ready(&mut client.hook.when_resolved()).is_err());
        let mut call = request(&client, 1);
        assert!(ready(&mut call.promise).is_err());
        assert!(hooks
            .poll_next(&mut Context::from_waker(Waker::noop()))
            .is_pending());
        hooks.close(graph, Error::disconnected("done".into()));
        graph.release(pending);
        assert_eq!(graph.take_retired(), Some(0));
    })
    .unwrap();
}

#[test]
fn capacity_counts_running_calls_and_cancellation_keeps_fifo() {
    clients::with_growing_graph::<u32, u8, _>(0, 1, |graph| {
        let route = graph.endpoint(7, Location::Local).unwrap();
        let mut hooks = ClientHooks::new(1, 2);
        let client = hooks.export_snapshot(graph, &route).unwrap();
        let canceled = request(&client, 1);
        let mut live = request(&client, 2);
        let mut rejected = request(&client, 3);
        assert_eq!(
            error(ready(&mut rejected.promise)).kind,
            capnp::ErrorKind::Overloaded
        );
        drop(canceled);
        let call = next(&mut hooks);
        assert_eq!(call.method_id(), 2);
        let mut later = request(&client, 4);
        let mut rejected = request(&client, 5);
        assert_eq!(
            error(ready(&mut rejected.promise)).kind,
            capnp::ErrorKind::Overloaded
        );
        // Removing a call from the inbox has not freed its admission permit.
        call.complete(Ok(()));
        ready(&mut live.promise).unwrap();
        let call = next(&mut hooks);
        assert_eq!(call.method_id(), 4);
        call.complete(Err(Error::failed("application fault".into())));
        assert_eq!(error(ready(&mut later.promise)).extra, "application fault");
        hooks.close(graph, Error::disconnected("done".into()));
        graph.release(route);
        assert_eq!(graph.take_retired(), Some(7));
        assert!(graph.is_empty());
    })
    .unwrap();
}

#[test]
fn cancellation_wakes_idle_inbox_and_running_call_without_io() {
    clients::with_growing_graph::<u32, u8, _>(0, 1, |graph| {
        let route = graph.endpoint(7, Location::Local).unwrap();
        let mut hooks = ClientHooks::new(1, 1);
        let client = hooks.export_snapshot(graph, &route).unwrap();
        let wakes = Arc::new(Wakes(AtomicUsize::new(0)));
        let waker = Waker::from(wakes.clone());
        let mut cx = Context::from_waker(&waker);
        assert!(hooks.poll_next(&mut cx).is_pending());
        let promise = request(&client, 1);
        assert_eq!(wakes.0.load(Ordering::Relaxed), 1);
        let mut call = next(&mut hooks);
        assert!(call.poll_canceled(&mut cx).is_pending());
        drop(promise);
        assert!(wakes.0.load(Ordering::Relaxed) > 1);
        assert!(call.is_canceled());
        assert!(call.poll_canceled(&mut cx).is_ready());
        drop(call);
        let canceled = request(&client, 2);
        let old = hooks.inbox.live_calls.get();
        drop(canceled);
        assert!(hooks.poll_next(&mut cx).is_pending());
        assert_eq!(old, 1);
        assert_eq!(hooks.inbox.live_calls.get(), 0);
        hooks.close(graph, Error::disconnected("done".into()));
        graph.release(route);
    })
    .unwrap();
}

#[test]
fn close_rejects_queued_running_and_escaped_clients() {
    let escaped = clients::with_growing_graph::<u32, u8, _>(0, 1, |graph| {
        let route = graph.endpoint(7, Location::Local).unwrap();
        let mut hooks = ClientHooks::new(1, 2);
        let client = hooks.export_snapshot(graph, &route).unwrap();
        let mut running = request(&client, 1);
        let mut call = next(&mut hooks);
        let mut queued = request(&client, 2);
        let wakes = Arc::new(Wakes(AtomicUsize::new(0)));
        let waker = Waker::from(wakes.clone());
        assert!(call
            .poll_canceled(&mut Context::from_waker(&waker))
            .is_pending());
        // The application has never polled either reply.
        hooks.close(graph, Error::disconnected("transport gone".into()));
        assert!(wakes.0.load(Ordering::Relaxed) > 0);
        assert!(call.is_canceled());
        assert!(call
            .poll_canceled(&mut Context::from_waker(Waker::noop()))
            .is_ready());
        call.complete(Ok(()));
        assert_eq!(error(ready(&mut running.promise)).extra, "transport gone");
        assert_eq!(error(ready(&mut queued.promise)).extra, "transport gone");
        graph.release(route);
        assert_eq!(graph.take_retired(), Some(7));
        assert!(graph.is_empty());
        client
    })
    .unwrap();
    let mut request = request(&escaped, 3);
    assert_eq!(
        error(ready(&mut request.promise)).kind,
        capnp::ErrorKind::Disconnected
    );
}

#[test]
fn foreign_inbox_cannot_authorize_call_and_terminal_drop_breaks_cycles() {
    clients::with_growing_graph::<u32, u8, _>(0, 1, |graph| {
        let route = graph.endpoint(7, Location::Local).unwrap();
        let mut hooks = ClientHooks::new(1, 1);
        let other = ClientHooks::new(1, 1);
        let client = hooks.export_snapshot(graph, &route).unwrap();
        let mut reply = request(&client, 1);
        let call = next(&mut hooks);
        assert_eq!(hooks.target(&call).unwrap().key(), route.key());
        assert_eq!(
            error(other.target(&call)).kind,
            capnp::ErrorKind::Disconnected
        );
        let weak = Rc::downgrade(&hooks.inbox);
        drop(hooks);
        assert!(weak.upgrade().is_none());
        assert_eq!(
            error(ready(&mut reply.promise)).kind,
            capnp::ErrorKind::Disconnected
        );
        assert!(call.is_canceled());
        drop(call);
        // Deliberate terminal path: graph scope reclaims unreleased refs.
        graph.release(route);
    })
    .unwrap();
}

#[test]
fn early_pipeline_and_capability_params_survive_parent_failure() {
    clients::with_growing_graph::<u32, u8, _>(0, 2, |graph| {
        let route = graph.endpoint(7, Location::Local).unwrap();
        let target = graph.endpoint(8, Location::Local).unwrap();
        let mut hooks = ClientHooks::new(2, 3);
        let client = hooks.export_snapshot(graph, &route).unwrap();
        let cap = hooks.export_snapshot(graph, &target).unwrap();
        let mut req = client.new_call::<any_pointer::Owned, any_pointer::Owned>(47, 1, None);
        req.get().set_as_capability(cap.hook.add_ref());
        let mut parent = req.send();
        let projected = capability::Client::new(parent.pipeline.as_cap());
        let mut child = request(&projected, 2);
        assert!(Pin::new(&mut child.promise)
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending());
        let mut call = next(&mut hooks);
        let received: capability::Client = call.params().unwrap().get_as_capability().unwrap();
        assert_eq!(received.hook.get_ptr(), cap.hook.get_ptr());
        let parameter = hooks.try_retain(graph, &*received.hook).unwrap().unwrap();
        assert_eq!(parameter.key(), target.key());
        graph.release(parameter);
        let mut pipeline = crate::PipelineBuilder::<any_pointer::Owned>::new();
        pipeline.get().set_as_capability(cap.hook.add_ref());
        call.results()
            .set_pipeline_from(pipeline.build().hook)
            .unwrap();
        call.complete(Err(Error::failed("late parent failure".into())));
        assert!(Pin::new(&mut child.promise)
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending());
        let mut child_call = next(&mut hooks);
        assert_eq!(hooks.target(&child_call).unwrap().key(), target.key());
        child_call
            .results()
            .get()
            .unwrap()
            .set_as::<capnp::text::Owned>("child ok")
            .unwrap();
        child_call.complete(Ok(()));
        let response = ready(&mut child.promise).unwrap();
        assert_eq!(
            response
                .get()
                .unwrap()
                .get_as::<capnp::text::Reader<'_>>()
                .unwrap(),
            "child ok"
        );
        assert_eq!(
            error(ready(&mut parent.promise)).extra,
            "late parent failure"
        );
        drop((client, cap, parent, child, projected, received, response));
        hooks.collect(graph);
        hooks.close(graph, Error::disconnected("done".into()));
        graph.release(route);
        graph.release(target);
        assert_eq!(graph.take_retired(), Some(7));
        assert_eq!(graph.take_retired(), Some(8));
        assert!(graph.is_empty());
    })
    .unwrap();
}

#[test]
fn prepared_pipeline_interest_survives_response_drop_before_admission() {
    for plain in [false, true] {
        for early in [false, true] {
            for fail in [false, true] {
                clients::with_growing_graph::<u32, u8, _>(0, 2, |graph| {
                    let route = graph.endpoint(7, Location::Local).unwrap();
                    let target = graph.endpoint(8, Location::Local).unwrap();
                    let mut hooks = ClientHooks::new(2, 3);
                    let client = hooks.export_snapshot(graph, &route).unwrap();
                    let cap = hooks.export_snapshot(graph, &target).unwrap();
                    let parent = request(&client, 1);
                    let projected = capability::Client::new(parent.pipeline.as_cap());
                    drop(parent);
                    let mut request =
                        projected.new_call::<any_pointer::Owned, any_pointer::Owned>(47, 2, None);
                    request.hook.set_hints(CallHints {
                        no_promise_pipelining: plain,
                        only_promise_pipeline: false,
                    });
                    let mut child = request.send();
                    let mut call = next(&mut hooks);
                    assert!(!call.is_canceled());
                    call.results()
                        .get()
                        .unwrap()
                        .set_as_capability(cap.hook.add_ref());
                    if early {
                        call.results().set_pipeline().unwrap();
                    }
                    call.complete(if fail {
                        Err(Error::failed("parent failure".into()))
                    } else {
                        Ok(())
                    });
                    let result =
                        Pin::new(&mut child.promise).poll(&mut Context::from_waker(Waker::noop()));
                    if fail && !early {
                        assert!(matches!(result, Poll::Ready(Err(_))));
                        assert!(hooks
                            .poll_next(&mut Context::from_waker(Waker::noop()))
                            .is_pending());
                    } else {
                        assert!(result.is_pending());
                        let mut child_call = next(&mut hooks);
                        assert_eq!(hooks.target(&child_call).unwrap().key(), target.key());
                        child_call
                            .results()
                            .get()
                            .unwrap()
                            .set_as("child result")
                            .unwrap();
                        child_call.complete(Ok(()));
                        assert_eq!(
                            ready(&mut child.promise)
                                .unwrap()
                                .get()
                                .unwrap()
                                .get_as::<capnp::text::Reader>()
                                .unwrap(),
                            "child result"
                        );
                    }
                    drop((child, projected, cap, client));
                    assert_eq!(hooks.inbox.live_calls.get(), 0);
                    hooks.collect(graph);
                    hooks.close(graph, Error::disconnected("done".into()));
                    graph.release(route);
                    graph.release(target);
                    while graph.take_retired().is_some() {}
                    assert!(graph.is_empty());
                })
                .unwrap();
            }
        }
    }
}

#[test]
fn prepared_pipeline_rejection_releases_credit_while_pipeline_is_retained() {
    clients::with_growing_graph::<u32, u8, _>(0, 1, |graph| {
        let route = graph.endpoint(7, Location::Remote(1)).unwrap();
        let admission = crate::admission::Admission::new(1);
        let mut hooks = ClientHooks::new(1, 0).with_admission(admission.clone());
        let client = hooks.export_snapshot(graph, &route).unwrap();
        let unsent = client.new_call::<any_pointer::Owned, any_pointer::Owned>(47, 1, None);
        assert_eq!(admission.used(), 1);
        let mut rejected = unsent.send();
        assert_eq!(admission.used(), 0);
        assert_eq!(
            error(ready(&mut rejected.promise)).kind,
            capnp::ErrorKind::Overloaded
        );
        drop((rejected, client));
        hooks.close(graph, Error::disconnected("done".into()));
        graph.release(route);
        while graph.take_retired().is_some() {}
        assert!(graph.is_empty());
    })
    .unwrap();
}

#[test]
fn prepared_pipeline_supports_late_projection_after_response_is_consumed() {
    clients::with_growing_graph::<u32, u8, _>(0, 2, |graph| {
        let route = graph.endpoint(7, Location::Local).unwrap();
        let target = graph.endpoint(8, Location::Local).unwrap();
        let mut hooks = ClientHooks::new(2, 3);
        let client = hooks.export_snapshot(graph, &route).unwrap();
        let cap = hooks.export_snapshot(graph, &target).unwrap();
        let mut parent = request(&client, 1);
        let mut call = next(&mut hooks);
        call.results()
            .get()
            .unwrap()
            .set_as_capability(cap.hook.add_ref());
        call.complete(Ok(()));
        drop(ready(&mut parent.promise).unwrap());
        drop(parent.promise);
        let projected = capability::Client::new(parent.pipeline.as_cap());
        drop(parent.pipeline);
        let mut child = request(&projected, 2);
        let child_call = next(&mut hooks);
        assert_eq!(hooks.target(&child_call).unwrap().key(), target.key());
        child_call.complete(Ok(()));
        ready(&mut child.promise).unwrap();
        drop((child, projected, cap, client));
        assert_eq!(hooks.inbox.live_calls.get(), 0);
        hooks.collect(graph);
        hooks.close(graph, Error::disconnected("done".into()));
        graph.release(route);
        graph.release(target);
        while graph.take_retired().is_some() {}
        assert!(graph.is_empty());
    })
    .unwrap();
}

#[test]
fn deferred_pipeline_requests_preserve_order_and_cancel_before_or_after_forwarding() {
    clients::with_growing_graph::<u32, u8, _>(0, 2, |graph| {
        let route = graph.endpoint(7, Location::Local).unwrap();
        let target = graph.endpoint(8, Location::Local).unwrap();
        let mut hooks = ClientHooks::new(2, 5);
        let client = hooks.export_snapshot(graph, &route).unwrap();
        let cap = hooks.export_snapshot(graph, &target).unwrap();
        let parent = request(&client, 1);
        let projected = capability::Client::new(parent.pipeline.as_cap());
        let send = |method, plain| {
            let mut call =
                projected.new_call::<any_pointer::Owned, any_pointer::Owned>(47, method, None);
            call.hook.set_hints(CallHints {
                no_promise_pipelining: plain,
                only_promise_pipeline: false,
            });
            call.get()
                .set_as::<capnp::text::Owned>("retained parameter")
                .unwrap();
            call.send()
        };
        let mut first = send(2, false);
        let mut second = send(3, true);
        drop(send(4, true));
        let mut last = send(5, true);
        let mut canceled_after_forwarding = Some(send(6, true));
        drop((parent, projected));
        let mut call = next(&mut hooks);
        call.results()
            .get()
            .unwrap()
            .set_as_capability(cap.hook.add_ref());
        call.complete(Ok(()));
        assert!(Pin::new(&mut second.promise)
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending());
        for method in [2, 3, 5, 6] {
            let mut call = next(&mut hooks);
            assert_eq!(call.method_id(), method);
            assert_eq!(hooks.target(&call).unwrap().key(), target.key());
            assert_eq!(
                call.params()
                    .unwrap()
                    .get_as::<capnp::text::Reader>()
                    .unwrap(),
                "retained parameter"
            );
            if method != 2 {
                assert!(matches!(call.completion, CallCompletion::Response(_)));
            }
            if method == 6 {
                // The request has forwarded after all public parent/projected
                // owners dropped. Its response still owns cancellation even
                // though forwarding borrowed the resolved target hook.
                drop(canceled_after_forwarding.take());
                assert!(call.is_canceled());
                call.complete(Err(Error::disconnected("canceled child".into())));
                continue;
            }
            call.results()
                .get()
                .unwrap()
                .set_as::<capnp::text::Owned>("reply")
                .unwrap();
            call.complete(Ok(()));
        }
        for promise in [&mut first.promise, &mut second.promise, &mut last.promise] {
            assert_eq!(
                ready(promise)
                    .unwrap()
                    .get()
                    .unwrap()
                    .get_as::<capnp::text::Reader>()
                    .unwrap(),
                "reply"
            );
        }
        assert!(hooks
            .poll_next(&mut Context::from_waker(Waker::noop()))
            .is_pending());
        drop((first, second, last, cap, client));
        assert_eq!(hooks.inbox.live_calls.get(), 0);
        hooks.collect(graph);
        hooks.close(graph, Error::disconnected("done".into()));
        graph.release(route);
        graph.release(target);
        while graph.take_retired().is_some() {}
        assert!(graph.is_empty());
    })
    .unwrap();
}

#[test]
fn prepared_pipeline_wakes_each_interest_and_cancels_after_the_last_drop() {
    for keep_response in [false, true] {
        clients::with_growing_graph::<u32, u8, _>(0, 2, |graph| {
            let route = graph.endpoint(7, Location::Local).unwrap();
            let target = graph.endpoint(8, Location::Local).unwrap();
            let mut hooks = ClientHooks::new(2, 3);
            let client = hooks.export_snapshot(graph, &route).unwrap();
            let cap = hooks.export_snapshot(graph, &target).unwrap();
            let parent = request(&client, 1);
            let projected = capability::Client::new(parent.pipeline.as_cap());
            let mut child = Some(request(&projected, 2));
            let mut response = Some(parent.promise);
            drop((parent.pipeline, projected));
            let response_wakes = Arc::new(Wakes(AtomicUsize::new(0)));
            let child_wakes = Arc::new(Wakes(AtomicUsize::new(0)));
            assert!(Pin::new(response.as_mut().unwrap())
                .poll(&mut Context::from_waker(&Waker::from(
                    response_wakes.clone()
                )))
                .is_pending());
            assert!(Pin::new(&mut child.as_mut().unwrap().promise)
                .poll(&mut Context::from_waker(&Waker::from(child_wakes.clone())))
                .is_pending());
            if keep_response {
                drop(child.take());
            } else {
                drop(response.take());
            }
            let mut call = next(&mut hooks);
            assert!(!call.is_canceled());
            call.results()
                .get()
                .unwrap()
                .set_as_capability(cap.hook.add_ref());
            call.complete(Ok(()));
            if keep_response {
                assert!(response_wakes.0.load(Ordering::Relaxed) > 0);
                let received = ready(response.as_mut().unwrap()).unwrap();
                let received: capability::Client =
                    received.get().unwrap().get_as_capability().unwrap();
                assert_eq!(received.hook.get_ptr(), cap.hook.get_ptr());
            } else {
                assert!(child_wakes.0.load(Ordering::Relaxed) > 0);
                assert!(Pin::new(&mut child.as_mut().unwrap().promise)
                    .poll(&mut Context::from_waker(Waker::noop()))
                    .is_pending());
                let child_call = next(&mut hooks);
                assert_eq!(hooks.target(&child_call).unwrap().key(), target.key());
                child_call.complete(Ok(()));
                ready(&mut child.as_mut().unwrap().promise).unwrap();
            }
            drop((response, child));
            let abandoned = request(&client, 3);
            let abandoned_call = next(&mut hooks);
            assert!(!abandoned_call.is_canceled());
            drop(abandoned.promise);
            assert!(!abandoned_call.is_canceled());
            drop(abandoned.pipeline);
            assert!(abandoned_call.is_canceled());
            drop((abandoned_call, cap, client));
            assert_eq!(hooks.inbox.live_calls.get(), 0);
            hooks.collect(graph);
            hooks.close(graph, Error::disconnected("done".into()));
            graph.release(route);
            graph.release(target);
            while graph.take_retired().is_some() {}
            assert!(graph.is_empty());
        })
        .unwrap();
    }
}

#[test]
fn request_hints_and_pipeline_only_interest_reach_worker() {
    clients::with_growing_graph::<u32, u8, _>(0, 1, |graph| {
        let route = graph.endpoint(7, Location::Local).unwrap();
        let mut hooks = ClientHooks::new(1, 2);
        let client = hooks.export_snapshot(graph, &route).unwrap();
        let req = client.new_call::<any_pointer::Owned, any_pointer::Owned>(47, 1, None);
        let pipeline = req.send_for_pipeline();
        let call = next(&mut hooks);
        assert!(call.hints().only_promise_pipeline);
        assert!(!call.hints().no_promise_pipelining);
        assert!(!call.is_canceled());
        drop(pipeline);
        assert!(call.is_canceled());
        drop(call);
        let hints = CallHints {
            no_promise_pipelining: true,
            only_promise_pipeline: false,
        };
        let mut reply = client
            .new_call_with_hints::<any_pointer::Owned, any_pointer::Owned>(47, 2, None, hints)
            .send();
        let call = next(&mut hooks);
        assert!(call.hints().no_promise_pipelining);
        call.complete(Ok(()));
        ready(&mut reply.promise).unwrap();
        hooks.close(graph, Error::disconnected("done".into()));
        graph.release(route);
    })
    .unwrap();
}

#[test]
fn queued_request_keeps_route_alive_after_all_application_clients_drop() {
    clients::with_growing_graph::<u32, u8, _>(0, 1, |graph| {
        let route = graph.endpoint(7, Location::Local).unwrap();
        let mut hooks = ClientHooks::new(1, 1);
        let client = hooks.export_snapshot(graph, &route).unwrap();
        let unsent = client.new_call::<any_pointer::Owned, any_pointer::Owned>(47, 1, None);
        drop(client);
        graph.release(route);
        hooks.collect(graph);
        assert!(graph.take_retired().is_none());
        let mut reply = unsent.send();
        let call = next(&mut hooks);
        hooks.collect(graph);
        assert!(matches!(
            graph.inspect(hooks.target(&call).unwrap()).unwrap(),
            Destination::Endpoint { owner: &7, .. }
        ));
        call.complete(Ok(()));
        ready(&mut reply.promise).unwrap();
        hooks.collect(graph);
        assert_eq!(graph.take_retired(), Some(7));
        assert!(graph.is_empty());
        hooks.close(graph, Error::disconnected("done".into()));
    })
    .unwrap();
}

#[test]
fn dispatch_backpressure_retries_oldest_call_without_duplicating_it() {
    let mut clients = [const { clients::Slot::EMPTY }; 1];
    let mut tasks = [const { dispatch::TaskSlot::EMPTY }; 1];
    dispatch::with_runtime(
        dispatch::Storage {
            clients: &mut clients,
            tasks: &mut tasks,
        },
        |runtime| {
            let route = runtime.endpoint(7, Location::<u8>::Local).unwrap();
            let mut hooks = ClientHooks::new(1, 3);
            let client = hooks.export_snapshot(runtime, &route).unwrap();
            let mut replies = (0..3)
                .map(|method| request(&client, method))
                .collect::<Vec<_>>();
            let mut cx = Context::from_waker(Waker::noop());
            assert!(matches!(
                hooks.poll_dispatch(runtime, &mut cx),
                Poll::Ready(Ok(Some(_)))
            ));
            for index in 0..3 {
                if index != 2 {
                    assert_eq!(
                        hooks.poll_dispatch(runtime, &mut cx),
                        Poll::Ready(Err(dispatch::DispatchError::Full))
                    );
                }
                let running = runtime.next_ready().unwrap().unwrap().accept();
                assert_eq!(running.owner().method_id(), index);
                runtime.complete(running).complete(Ok(()));
                ready(&mut replies[index as usize].promise).unwrap();
                if index != 2 {
                    assert!(matches!(
                        hooks.poll_dispatch(runtime, &mut cx),
                        Poll::Ready(Ok(Some(_)))
                    ));
                }
            }
            assert!(runtime.next_ready().unwrap().is_none());
            assert!(hooks.poll_dispatch(runtime, &mut cx).is_pending());
            hooks.close(runtime, Error::disconnected("done".into()));
            runtime.release(route);
        },
    )
    .unwrap();
}

// It is legal for a membrane to preserve identity diagnostics. It must not
// accidentally confer access to a bypass around its interception policy.
pub(super) struct Opaque(pub(super) Box<dyn ClientHook>);
impl ClientHook for Opaque {
    fn add_ref(&self) -> Box<dyn ClientHook> {
        Box::new(Self(self.0.add_ref()))
    }
    fn new_call(
        &self,
        interface: u64,
        method: u16,
        size: Option<capnp::MessageSize>,
    ) -> capability::Request<any_pointer::Owned, any_pointer::Owned> {
        self.0.new_call(interface, method, size)
    }
    fn call(
        &self,
        interface: u64,
        method: u16,
        params: Box<dyn ParamsHook>,
        results: Box<dyn ResultsHook>,
    ) -> Promise<(), Error> {
        self.0.call(interface, method, params, results)
    }
    fn get_brand(&self) -> usize {
        self.0.get_brand()
    }
    fn get_ptr(&self) -> usize {
        self.0.get_ptr()
    }
    fn get_resolved(&self) -> Option<Box<dyn ClientHook>> {
        None
    }
    fn when_more_resolved(&self) -> Option<Promise<Box<dyn ClientHook>, Error>> {
        None
    }
    fn when_resolved(&self) -> Promise<(), Error> {
        Promise::ok(())
    }
}

#[test]
fn parameter_lookup_preserves_opaque_policy_and_checks_connection_identity() {
    clients::with_growing_graph::<u32, u8, _>(0, 1, |graph| {
        let route = graph.endpoint(7, Location::Local).unwrap();
        let mut hooks = ClientHooks::new(1, 1);
        let mut foreign = ClientHooks::new(1, 1);
        let client = hooks.export_snapshot(graph, &route).unwrap();
        let other = foreign.export_snapshot(graph, &route).unwrap();
        let opaque = Opaque(client.hook.add_ref());
        assert_eq!(opaque.get_ptr(), client.hook.get_ptr());
        assert_eq!(opaque.get_brand(), client.hook.get_brand());
        assert!(hooks.try_retain(graph, &opaque).unwrap().is_none());
        assert!(hooks.try_retain(graph, &*other.hook).unwrap().is_none());
        let retained = hooks.try_retain(graph, &*client.hook).unwrap().unwrap();
        assert_eq!(retained.key(), route.key());
        graph.release(retained);
        hooks.close(graph, Error::disconnected("done".into()));
        foreign.close(graph, Error::disconnected("done".into()));
        graph.release(route);
        assert_eq!(graph.take_retired(), Some(7));
        assert!(graph.is_empty());
    })
    .unwrap();
}

#[test]
fn returned_capability_keeps_graph_route_until_final_response_is_dropped() {
    clients::with_growing_graph::<u32, u8, _>(0, 2, |graph| {
        let route = graph.endpoint(7, Location::Local).unwrap();
        let target = graph.endpoint(8, Location::Local).unwrap();
        let mut hooks = ClientHooks::new(2, 1);
        let client = hooks.export_snapshot(graph, &route).unwrap();
        let cap = hooks.export_snapshot(graph, &target).unwrap();
        let mut reply = request(&client, 1);
        let mut call = next(&mut hooks);
        call.results().get().unwrap().set_as_capability(cap.hook);
        graph.release(target);
        call.complete(Ok(()));
        let response = ready(&mut reply.promise).unwrap();
        drop(reply);
        hooks.collect(graph);
        assert!(graph.take_retired().is_none());
        let received: capability::Client = response.get().unwrap().get_as_capability().unwrap();
        let retained = hooks.try_retain(graph, &*received.hook).unwrap().unwrap();
        assert!(matches!(
            graph.inspect(&retained).unwrap(),
            Destination::Endpoint { owner: &8, .. }
        ));
        graph.release(retained);
        drop(response);
        hooks.collect(graph);
        assert!(graph.take_retired().is_none());
        drop(received);
        hooks.collect(graph);
        assert_eq!(graph.take_retired(), Some(8));
        hooks.close(graph, Error::disconnected("done".into()));
        graph.release(route);
        assert_eq!(graph.take_retired(), Some(7));
        assert!(graph.is_empty());
    })
    .unwrap();
}

thread_local! { static REENTER: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None); }
struct Reenter;
impl std::task::Wake for Reenter {
    fn wake(self: Arc<Self>) {
        let action = REENTER.with(|slot| slot.borrow_mut().take());
        if let Some(action) = action {
            action();
        }
    }
}

#[test]
fn wake_callback_can_enqueue_and_drop_clients_without_borrow_panics_or_reordering() {
    clients::with_growing_graph::<u32, u8, _>(0, 1, |graph| {
        let route = graph.endpoint(7, Location::Local).unwrap();
        let mut hooks = ClientHooks::new(1, 2);
        let client = hooks.export_snapshot(graph, &route).unwrap();
        let nested_client = client.clone();
        let nested = Rc::new(RefCell::new(None));
        let nested_result = nested.clone();
        REENTER.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(move || {
                *nested_result.borrow_mut() = Some(request(&nested_client, 2));
                drop(nested_client);
            }))
        });
        let waker = Waker::from(Arc::new(Reenter));
        assert!(hooks
            .poll_next(&mut Context::from_waker(&waker))
            .is_pending());
        let mut outer = request(&client, 1);
        let call = next(&mut hooks);
        assert_eq!(call.method_id(), 1);
        call.complete(Ok(()));
        ready(&mut outer.promise).unwrap();
        let call = next(&mut hooks);
        assert_eq!(call.method_id(), 2);
        call.complete(Ok(()));
        ready(&mut nested.borrow_mut().as_mut().unwrap().promise).unwrap();
        hooks.close(graph, Error::disconnected("done".into()));
        graph.release(route);
    })
    .unwrap();
}

#[test]
fn zero_limits_reject_before_retaining_or_admitting_work() {
    clients::with_growing_graph::<u32, u8, _>(0, 1, |graph| {
        let route = graph.endpoint(7, Location::Local).unwrap();
        let mut no_clients = ClientHooks::new(0, 1);
        assert_eq!(
            error(no_clients.export_snapshot(graph, &route)).kind,
            capnp::ErrorKind::Overloaded
        );
        let mut no_calls = ClientHooks::new(1, 0);
        let client = no_calls.export_snapshot(graph, &route).unwrap();
        let mut reply = request(&client, 1);
        assert_eq!(
            error(ready(&mut reply.promise)).kind,
            capnp::ErrorKind::Overloaded
        );
        assert!(no_calls
            .poll_next(&mut Context::from_waker(Waker::noop()))
            .is_pending());
        no_clients.close(graph, Error::disconnected("done".into()));
        no_calls.close(graph, Error::disconnected("done".into()));
        graph.release(route);
        assert_eq!(graph.take_retired(), Some(7));
        assert!(graph.is_empty());
    })
    .unwrap();
}

#[test]
fn unpolled_streaming_send_cannot_be_overtaken_by_ordinary_send() {
    clients::with_growing_graph::<u32, u8, _>(0, 1, |graph| {
        let route = graph.endpoint(7, Location::Local).unwrap();
        let mut hooks = ClientHooks::new(1, 2);
        let client = hooks.export_snapshot(graph, &route).unwrap();
        let mut stream = client
            .new_streaming_call::<any_pointer::Owned>(47, 1, None)
            .send();
        let mut ordinary = request(&client, 2);
        let call = next(&mut hooks);
        assert_eq!(
            call.method_id(),
            1,
            "send order must not depend on future poll order"
        );
        call.complete(Ok(()));
        ready(&mut stream).unwrap();
        let call = next(&mut hooks);
        assert_eq!(call.method_id(), 2);
        call.complete(Ok(()));
        ready(&mut ordinary.promise).unwrap();
        hooks.close(graph, Error::disconnected("done".into()));
        graph.release(route);
    })
    .unwrap();
}

#[test]
fn hosted_endpoint_metadata_preserves_broken_error_kind_and_reason() {
    clients::with_growing_graph::<Error, u8, _>(0, 4, |graph| {
        let pending = graph.promise(&mut None).unwrap();
        let mut hooks = ClientHooks::new(4, 4);
        let client = hooks.export_client(graph, &pending).unwrap();
        let mut settled = client.hook.when_more_resolved().unwrap();
        let broken = graph
            .endpoint(
                Error::overloaded("peer queue full".into()),
                Location::Broken,
            )
            .unwrap();
        graph
            .resolve(pending.key(), &mut Some(broken), clients::Source::Direct)
            .unwrap();
        hooks.collect_with_metadata(graph, |owner| EndpointMetadata {
            failure: Some(owner.clone()),
            ..EndpointMetadata::default()
        });
        let failure = error(ready(&mut settled));
        assert_eq!(failure.kind, capnp::ErrorKind::Overloaded);
        assert_eq!(failure.extra, "peer queue full");
        hooks.collect(graph);
        let mut call = request(&client, 0);
        let failure = error(ready(&mut call.promise));
        assert_eq!(failure.kind, capnp::ErrorKind::Overloaded);
        assert_eq!(failure.extra, "peer queue full");
        drop(client);
        hooks.close(graph, Error::disconnected("done".into()));
        graph.release(pending);
    })
    .unwrap();
}

#[test]
fn response_context_callbacks_cannot_bypass_capacity_or_enqueue_after_close() {
    struct Params;
    impl ParamsHook for Params {
        fn get(&self) -> capnp::Result<any_pointer::Reader<'_>> {
            panic!("not dispatched")
        }
    }
    struct Results(Option<Box<dyn FnOnce()>>);
    impl ResultsHook for Results {
        fn retain_response_context(&mut self, _: Rc<dyn std::any::Any>) {
            if let Some(callback) = self.0.take() {
                callback();
            }
        }
        fn get_with_size_hint(
            &mut self,
            _: Option<capnp::MessageSize>,
        ) -> capnp::Result<any_pointer::Builder<'_>> {
            panic!("not dispatched")
        }
        fn set_pipeline(&mut self) -> capnp::Result<()> {
            panic!("not dispatched")
        }
        fn set_pipeline_from(
            &mut self,
            _: Box<dyn capnp::private::capability::PipelineHook>,
        ) -> capnp::Result<()> {
            panic!("not dispatched")
        }
        fn allow_cancellation(&self) {}
        fn tail_call(
            self: Box<Self>,
            _: Box<dyn capnp::private::capability::RequestHook>,
        ) -> Promise<(), Error> {
            panic!("not dispatched")
        }
        fn direct_tail_call(
            self: Box<Self>,
            _: Box<dyn capnp::private::capability::RequestHook>,
        ) -> (
            Promise<(), Error>,
            Box<dyn capnp::private::capability::PipelineHook>,
        ) {
            panic!("not dispatched")
        }
    }
    clients::with_growing_graph::<u32, u8, _>(0, 1, |graph| {
        let route = graph.endpoint(1, Location::Local).unwrap();
        let admission = crate::admission::Admission::new(1);
        let mut hooks = ClientHooks::new(1, 1).with_admission(admission);
        let client = hooks.export_snapshot(graph, &route).unwrap();
        let nested = client.clone();
        let inbox = hooks.inbox.clone();
        let callback = move || {
            let mut rejected = nested
                .hook
                .call(0, 0, Box::new(Params), Box::new(Results(None)));
            assert_eq!(
                ready(&mut rejected).unwrap_err().kind,
                capnp::ErrorKind::Overloaded
            );
            assert_eq!(inbox.live_calls.get(), 1);
            inbox.close(Error::disconnected("closed by callback".into()));
        };
        let mut rejected = client.hook.call(
            0,
            0,
            Box::new(Params),
            Box::new(Results(Some(Box::new(callback)))),
        );
        assert_eq!(
            ready(&mut rejected).unwrap_err().kind,
            capnp::ErrorKind::Disconnected
        );
        assert_eq!(hooks.inbox.live_calls.get(), 0);
        assert!(hooks.inbox.queue.borrow().is_empty());
        drop(client);
        hooks.close(graph, Error::disconnected("done".into()));
        graph.release(route);
    })
    .unwrap();
}

#[test]
fn prepared_method_headers_preserve_parameters_after_cache_saturation() {
    clients::with_growing_graph::<u32, u8, _>(0, 1, |graph| {
        let route = graph.endpoint(7, Location::Remote(1)).unwrap();
        let mut hooks = ClientHooks::new(1, 1);
        let client = hooks.export_snapshot(graph, &route).unwrap();
        for index in (0..8).chain((0..8).rev()) {
            let interface = 47 + index / 3;
            let method = (index % 3) as u16;
            let text = format!("method {index}");
            let mut request = client.hook.new_call(interface, method, None);
            request.get().set_as::<capnp::text::Owned>(&text).unwrap();
            let mut reply = request.send();
            let call = next(&mut hooks);
            let message = call.params.prepared_call().unwrap().message();
            let crate::rpc_capnp::message::Call(wire) = message
                .get_root_as_reader::<crate::rpc_capnp::message::Reader>()
                .unwrap()
                .which()
                .unwrap()
            else {
                panic!("prepared Call");
            };
            let wire = wire.unwrap();
            assert_eq!(
                (wire.get_interface_id(), wire.get_method_id()),
                (interface, method)
            );
            assert_eq!(
                wire.get_params()
                    .unwrap()
                    .get_content()
                    .get_as::<capnp::text::Reader>()
                    .unwrap(),
                text
            );
            call.complete(Ok(()));
            ready(&mut reply.promise).unwrap();
        }
        drop(client);
        graph.release(route);
        hooks.collect(graph);
        assert_eq!(graph.take_retired(), Some(7));
        assert!(graph.is_empty());
    })
    .unwrap();
}

#[test]
fn prepared_request_keeps_its_allocation_through_admission_and_final_hint_changes() {
    for change_hints in [false, true] {
        clients::with_growing_graph::<u32, u8, _>(0, 4, |graph| {
            let route = graph.endpoint(7, Location::Remote(1)).unwrap();
            let mut hooks = ClientHooks::new(1, 2);
            let client = hooks.export_snapshot(graph, &route).unwrap();
            let mut request = client.hook.new_call_with_hints(
                47,
                3,
                None,
                CallHints {
                    no_promise_pipelining: !change_hints,
                    only_promise_pipeline: false,
                },
            );
            request.get().set_as("owned parameters").unwrap();
            if change_hints {
                request.hook.set_hints(CallHints {
                    no_promise_pipelining: true,
                    only_promise_pipeline: false,
                });
            }
            let address = (&*request.hook as *const dyn capnp::private::capability::RequestHook)
                .cast::<()>() as usize;
            let mut reply = request.send();
            let Poll::Ready(Some(call)) =
                hooks.poll_owned_next(&mut Context::from_waker(Waker::noop()))
            else {
                panic!("admitted request")
            };
            assert_eq!((&*call as *const Call) as usize, address);
            assert_eq!(
                call.params()
                    .unwrap()
                    .get_as::<capnp::text::Reader>()
                    .unwrap(),
                "owned parameters"
            );
            assert_eq!((call.interface_id(), call.method_id()), (47, 3));
            assert!(call.hints().no_promise_pipelining);
            call.complete_boxed(Err(Error::failed("completed".into())));
            assert_eq!(error(ready(&mut reply.promise)).extra, "completed");
            assert_eq!(hooks.inbox.live_calls.get(), 0);
            drop((reply, client));
            hooks.collect(graph);
            graph.release(route);
            hooks.close(graph, Error::disconnected("done".into()));
        })
        .unwrap();
    }
}

#[test]
fn forwarding_preserves_parameters_results_errors_and_releases_both_call_permits() {
    for plain in [false, true] {
        for fail in [false, true] {
            clients::with_growing_graph::<u32, u8, _>(0, 4, |graph| {
                let source_route = graph.endpoint(1, Location::Local).unwrap();
                let target_route = graph.endpoint(2, Location::Local).unwrap();
                let mut source = ClientHooks::new(1, 1);
                let mut target = ClientHooks::new(1, 1);
                let client = source.export_snapshot(graph, &source_route).unwrap();
                let destination = target.export_snapshot(graph, &target_route).unwrap();
                let mut request = client.hook.new_call_with_hints(
                    47,
                    3,
                    None,
                    CallHints {
                        no_promise_pipelining: plain,
                        only_promise_pipeline: false,
                    },
                );
                request.get().set_as("forwarded parameters").unwrap();
                let mut reply = request.send();
                let mut forwarding = next(&mut source).forward(&*destination.hook);
                assert_eq!(source.inbox.live_calls.get(), 1);
                let mut call = next(&mut target);
                assert_eq!((call.interface_id(), call.method_id()), (47, 3));
                assert_eq!(call.hints().no_promise_pipelining, plain);
                assert_eq!(
                    call.params()
                        .unwrap()
                        .get_as::<capnp::text::Reader>()
                        .unwrap(),
                    "forwarded parameters"
                );
                call.results()
                    .get()
                    .unwrap()
                    .set_as("forwarded result")
                    .unwrap();
                call.complete(if fail {
                    Err(Error::failed("forwarded failure".into()))
                } else {
                    Ok(())
                });
                let completion = ready(&mut forwarding);
                if fail {
                    assert_eq!(completion.unwrap_err().extra, "forwarded failure");
                    assert_eq!(error(ready(&mut reply.promise)).extra, "forwarded failure");
                } else {
                    completion.unwrap();
                    assert_eq!(
                        ready(&mut reply.promise)
                            .unwrap()
                            .get()
                            .unwrap()
                            .get_as::<capnp::text::Reader>()
                            .unwrap(),
                        "forwarded result"
                    );
                }
                assert_eq!(source.inbox.live_calls.get(), 0);
                assert_eq!(target.inbox.live_calls.get(), 0);
                drop((forwarding, reply, client, destination));
                source.close(graph, Error::disconnected("done".into()));
                target.close(graph, Error::disconnected("done".into()));
                graph.release(source_route);
                graph.release(target_route);
                assert_eq!(graph.take_retired(), Some(1));
                assert_eq!(graph.take_retired(), Some(2));
                assert!(graph.is_empty());
            })
            .unwrap();
        }
    }
}

#[test]
fn general_request_fallback_preserves_envelopes_and_direct_response_ownership() {
    use capnp::private::capability::RequestHook;
    for envelope in [false, true] {
        clients::with_growing_graph::<u32, u8, _>(0, 2, |graph| {
            let route = graph.endpoint(7, Location::Remote(1)).unwrap();
            let mut hooks = ClientHooks::new(1, 1);
            let client = hooks.export_snapshot(graph, &route).unwrap();
            let hints = CallHints {
                no_promise_pipelining: true,
                only_promise_pipeline: false,
            };
            let mut request = if envelope {
                let hook = client
                    .hook
                    .as_any()
                    .unwrap()
                    .downcast_ref::<Hook>()
                    .unwrap();
                Box::new(
                    crate::local::Params::scoped(
                        47,
                        3,
                        capnp::message::HeapAllocator::new(),
                        Some(None),
                    )
                    .into_request(
                        47,
                        3,
                        hints,
                        Hook(hook.0.clone(), hook.1.clone()),
                        None,
                        None,
                    ),
                )
            } else {
                let mut request = Box::new(crate::local::Request::new(
                    47,
                    3,
                    None,
                    client.hook.add_ref(),
                ));
                request.set_hints(hints);
                request
            };
            RequestHook::get(&mut *request)
                .set_as("fallback parameters")
                .unwrap();
            assert_eq!(request.prepared_call().is_some(), envelope);
            assert!(ParamsHook::as_any(&*request)
                .unwrap()
                .is::<crate::local::Request>());
            assert_eq!(
                ParamsHook::get(&*request)
                    .unwrap()
                    .get_as::<capnp::text::Reader>()
                    .unwrap(),
                "fallback parameters"
            );
            let mut reply = request.send();
            let mut call = next(&mut hooks);
            assert_eq!(
                call.params()
                    .unwrap()
                    .get_as::<capnp::text::Reader>()
                    .unwrap(),
                "fallback parameters"
            );
            assert_eq!((call.interface_id(), call.method_id()), (47, 3));
            call.results()
                .get()
                .unwrap()
                .set_as("fallback result")
                .unwrap();
            call.complete(Ok(()));
            assert_eq!(
                ready(&mut reply.promise)
                    .unwrap()
                    .get()
                    .unwrap()
                    .get_as::<capnp::text::Reader>()
                    .unwrap(),
                "fallback result"
            );
            assert_eq!(hooks.inbox.live_calls.get(), 0);
            drop((reply, client));
            hooks.close(graph, Error::disconnected("done".into()));
            graph.release(route);
            assert_eq!(graph.take_retired(), Some(7));
            assert!(graph.is_empty());
        })
        .unwrap();
    }
}

#[test]
fn hook_index_preserves_converged_aliases_and_recycled_generations() {
    clients::with_growing_graph::<(), u8, _>(0, 8, |graph| {
        let mut hooks = ClientHooks::new(3, 1);
        let terminal = graph.endpoint((), Location::Local).unwrap();
        let a = graph.promise(&mut None).unwrap();
        let b = graph.promise(&mut None).unwrap();
        let a_hook = hooks.export_client(graph, &a).unwrap();
        let b_hook = hooks.export_client(graph, &b).unwrap();
        for promise in [&a, &b] {
            let mut target = Some(graph.retain_key(terminal.key()).unwrap());
            graph
                .resolve(promise.key(), &mut target, clients::Source::Direct)
                .unwrap();
        }
        hooks.collect(graph);
        assert_eq!(hooks.bindings.len(), 1);
        let head = hooks.export_client(graph, &terminal).unwrap();
        assert_eq!(head.hook.get_ptr(), b_hook.hook.get_ptr());
        drop((head, b_hook));
        hooks.reclaim_released(graph);
        let remaining = hooks.export_client(graph, &terminal).unwrap();
        assert_eq!(remaining.hook.get_ptr(), a_hook.hook.get_ptr());
        drop((remaining, a_hook));
        hooks.reclaim_released(graph);
        assert!(hooks.bindings.is_empty());
        graph.release(a);
        graph.release(b);
        graph.release(terminal);
        while graph.take_retired().is_some() {}
        let reused = graph.endpoint((), Location::Local).unwrap();
        let fresh = hooks.export_client(graph, &reused).unwrap();
        assert_eq!(hooks.bindings.len(), 1);
        drop(fresh);
        hooks.reclaim_released(graph);
        graph.release(reused);
        while graph.take_retired().is_some() {}
        assert!(graph.is_empty());
    })
    .unwrap();
}
