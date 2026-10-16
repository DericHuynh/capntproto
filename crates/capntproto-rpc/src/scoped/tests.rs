use super::*;
use capnp::capability::FromClientHook;
use capntproto_rpc_engine::clients::{self, dispatch, Destination, Location};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

type Remote = capability::RemotePromise<any_pointer::Owned>;
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
                let mut child = request(&projected, 2);
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
