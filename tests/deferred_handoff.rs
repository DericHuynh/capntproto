#[allow(dead_code)]
mod support;
use capnp::capability::FromClientHook;
use capnp_rpc::RpcSystem;
use reproto_test_support::runtime_test_capnp::harness;
use std::{cell::RefCell, rc::Rc, time::Duration};
use support::Hub;

type Slot = Rc<RefCell<Option<harness::Client>>>;
struct Service {
    slot: Slot,
    calls: Rc<RefCell<Vec<u32>>>,
}
impl harness::Server for Service {
    async fn echo(
        self: Rc<Self>,
        p: harness::EchoParams,
        mut r: harness::EchoResults,
    ) -> capnp::Result<()> {
        let value = p.get()?.get_value();
        self.calls.borrow_mut().push(value);
        r.get().set_value(value);
        Ok(())
    }
    async fn bounce(
        self: Rc<Self>,
        p: harness::BounceParams,
        mut r: harness::BounceResults,
    ) -> capnp::Result<()> {
        let cap = p.get()?.get_cap()?;
        *self.slot.borrow_mut() = Some(cap.clone());
        r.get().set_cap(cap);
        Ok(())
    }
}
async fn drain() {
    for _ in 0..40 {
        tokio::task::yield_now().await;
    }
}
async fn send(to: &harness::Client, cap: harness::Client) -> harness::Client {
    let mut request = to.bounce_request();
    request.get().set_cap(cap);
    request
        .send()
        .promise
        .await
        .unwrap()
        .get()
        .unwrap()
        .get_cap()
        .unwrap()
}
async fn use_cap(cap: &harness::Client, value: u32) {
    let mut request = cap.echo_request();
    request.get().set_value(value);
    assert_eq!(
        request
            .send()
            .promise
            .await
            .unwrap()
            .get()
            .unwrap()
            .get_value(),
        value
    );
}
struct Setup {
    hub: Rc<RefCell<Hub>>,
    slots: Vec<Slot>,
    calls: Rc<RefCell<Vec<u32>>>,
    bc: harness::Client,
    cd: harness::Client,
    bd: harness::Client,
    local_owner: harness::Client,
    tasks: Vec<tokio::task::JoinHandle<capnp::Result<()>>>,
}
impl Drop for Setup {
    fn drop(&mut self) {
        // Test services retain received clients, including clients to themselves.
        // Release these application-owned cycles before shutting down the vats.
        for slot in &self.slots {
            let cap = slot.borrow_mut().take();
            drop(cap);
        }
        for task in &self.tasks {
            task.abort();
        }
    }
}
impl Setup {
    async fn new(forwarding: bool) -> Self {
        Self::configured(forwarding, false, false).await
    }
    async fn configured(forwarding: bool, local: bool, reject: bool) -> Self {
        let hub = Rc::new(RefCell::new(Hub::default()));
        hub.borrow_mut().introductions = true;
        hub.borrow_mut().forwarding = forwarding;
        hub.borrow_mut().reject_accept = reject;
        let calls = Rc::new(RefCell::new(Vec::new()));
        let slots: Vec<Slot> = (0..5).map(|_| Rc::new(RefCell::new(None))).collect();
        let mut systems: Vec<_> = (0..5)
            .map(|id| {
                let network = Hub::network(&hub, id);
                let service = Service {
                    slot: slots[id as usize].clone(),
                    calls: calls.clone(),
                };
                #[cfg(unix)]
                let server: harness::Client = if id == 0 {
                    use std::io::Write;
                    let mut file = tempfile::tempfile().unwrap();
                    file.write_all(b"handoff fd").unwrap();
                    capnp_rpc::new_fd_client(service, file.into())
                } else {
                    capnp_rpc::new_client(service)
                };
                #[cfg(not(unix))]
                let server: harness::Client = capnp_rpc::new_client(service);
                RpcSystem::new(Box::new(network), Some(server.client))
            })
            .collect();
        // 0 = host, 1 = introducer, 2 = B, 3 = C, 4 = D.
        let local_owner: harness::Client = systems[0].bootstrap(0);
        let owner: harness::Client = systems[1].bootstrap(0);
        let ab: harness::Client = systems[1].bootstrap(2);
        let bc = systems[2].bootstrap(3);
        let bd = systems[2].bootstrap(if local { 0 } else { 4 });
        let cd = systems[3].bootstrap(if local { 0 } else { 4 });
        let tasks = systems.into_iter().map(tokio::task::spawn_local).collect();
        let owner = capnp::capability::get_resolved_cap(owner).await;
        drop(send(&ab, owner).await);
        drain().await;
        assert_eq!(hub.borrow().introduction_count, 1);
        assert_eq!(hub.borrow().accept_count, 0);
        assert_eq!(hub.borrow().provision_count(), 1);
        Self {
            hub,
            slots,
            calls,
            bc,
            cd,
            bd,
            local_owner,
            tasks,
        }
    }
    fn cap(&self, id: usize) -> harness::Client {
        self.slots[id].borrow().as_ref().unwrap().clone()
    }
    fn drop_cap(&self, id: usize) {
        self.slots[id].borrow_mut().take();
    }
}

#[tokio::test(flavor = "current_thread")]
async fn diagnostic_inspection_never_accepts_a_deferred_handoff() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                let setup = Setup::new(true).await;
                let cap = setup.cap(2);
                let identity = cap.as_client_hook().get_ptr();
                for _ in 0..4 {
                    assert!(cap.debug_info().contains("thirdPartyPending"));
                    assert_eq!(cap.as_client_hook().get_ptr(), identity);
                }
                drain().await;
                assert_eq!(setup.hub.borrow().accept_count, 0);
                assert_eq!(setup.hub.borrow().introduction_count, 1);
                assert_eq!(setup.hub.borrow().provision_count(), 1);
                assert!(setup.calls.borrow().is_empty());
                use_cap(&cap, 77).await;
                assert_eq!(setup.hub.borrow().accept_count, 1);
                assert!(cap.debug_info().contains("thirdPartyAccepted"));
                assert_eq!(*setup.calls.borrow(), [77]);
            })
            .await
            .unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn forwarding_reflection_and_multiple_accepts_keep_original_provision() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                let s = Setup::new(true).await;
                // Returning a forwarded contact must preserve its ability to be forwarded.
                let reflected = send(&s.bc, s.cap(2)).await;
                let original = s.cap(2);
                s.drop_cap(2);
                drop(send(&s.bd, reflected).await);
                drain().await;
                assert_eq!(s.hub.borrow().forward_count, 2);
                assert_eq!(s.hub.borrow().accept_count, 0);
                assert_eq!(s.hub.borrow().introduction_count, 1);
                // Two forwarded recipients and the original recipient share a provision.
                // Their embargo IDs must not collide, even though each is its first Accept.
                use_cap(&s.cap(4), 4).await;
                use_cap(&s.cap(3), 3).await;
                use_cap(&original, 2).await;
                use_cap(&original, 20).await;
                drain().await;
                assert_eq!(s.hub.borrow().accept_count, 3);
                assert_eq!(*s.calls.borrow(), vec![4, 3, 2, 20]);
                assert_eq!(s.hub.borrow().provision_count(), 0);
            })
            .await
            .unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn forwarding_chain_keeps_vines_until_final_accept_and_then_releases() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                let s = Setup::new(true).await;
                drop(send(&s.bc, s.cap(2)).await);
                s.drop_cap(2);
                drop(send(&s.cd, s.cap(3)).await);
                s.drop_cap(3);
                drain().await;
                assert_eq!(s.hub.borrow().accept_count, 0);
                assert_eq!(s.hub.borrow().provision_count(), 1);
                use_cap(&s.cap(4), 42).await;
                drain().await;
                assert_eq!(s.hub.borrow().accept_count, 1);
                assert_eq!(s.hub.borrow().forward_count, 2);
                assert_eq!(s.hub.borrow().provision_count(), 0);
                // Host identity converges through repeated local use.
                let a = capnp::capability::get_resolved_cap(s.cap(4)).await;
                let b = capnp::capability::get_resolved_cap(s.cap(4)).await;
                assert_eq!(
                    a.into_client_hook().get_ptr(),
                    b.into_client_hook().get_ptr()
                );
            })
            .await
            .unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn network_without_contact_forwarding_accepts_then_reintroduces() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                let s = Setup::new(false).await;
                drop(send(&s.bc, s.cap(2)).await);
                use_cap(&s.cap(3), 9).await;
                drain().await;
                assert_eq!(s.hub.borrow().forward_count, 0);
                assert_eq!(s.hub.borrow().accept_count, 2);
                assert_eq!(s.hub.borrow().introduction_count, 2);
                assert_eq!(*s.calls.borrow(), vec![9]);
            })
            .await
            .unwrap();
        })
        .await;
}

#[derive(serde::Deserialize, Debug)]
struct Step {
    action: String,
    state: Vec<u64>,
}
#[derive(serde::Deserialize, Debug)]
struct Trace {
    reflection: bool,
    local: bool,
    reject: bool,
    reject_forward: bool,
    steps: Vec<Step>,
}
async fn replay(trace: &Trace) {
    let s = Setup::configured(true, trace.local, trace.reject).await;
    let last = if trace.local { 0 } else { 4 };
    s.hub.borrow_mut().reject_forward = trace.reject_forward;
    let mut reflected = None;
    let mut used = [false; 4];
    let mut forwards = [false; 2];
    let mut expected_calls = Vec::new();
    // Optimistic resolution queries and cloning are not local use.
    assert!(s.cap(2).into_client_hook().get_resolved().is_none());
    #[cfg(unix)]
    assert!(s.cap(2).client.hook.get_fd().is_none());
    for step in &trace.steps {
        match step.action.as_str() {
            "forward-b" => {
                let cap = send(&s.bc, s.cap(2)).await;
                if trace.reflection {
                    reflected = Some(cap);
                }
                forwards[0] = true;
            }
            "forward-next" => {
                drop(if trace.reflection {
                    send(&s.bd, reflected.as_ref().unwrap().clone()).await
                } else {
                    send(&s.cd, s.cap(3)).await
                });
                forwards[1] = true;
            }
            "use-b" | "use-c" | "use-d" | "use-r" => {
                let id = match step.action.as_str() {
                    "use-b" => 0,
                    "use-c" => 1,
                    "use-d" => 2,
                    _ => 3,
                };
                let cap = if id == 3 {
                    reflected.as_ref().unwrap().clone()
                } else {
                    s.cap(if id == 2 { last } else { id + 2 })
                };
                let forward_error = trace.reject_forward && id != 0;
                if trace.reject || forward_error {
                    // An error is cached: repeated calls must not try to accept again.
                    for _ in 0..2 {
                        let error = cap.echo_request().send().promise.await.err().unwrap();
                        assert!(error.extra.contains(if forward_error {
                            "forwarding rejected"
                        } else {
                            "introduction rejected"
                        }));
                    }
                } else {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::FileExt;
                        // get_fd() is a use which forces acceptance; optimistic
                        // hook queries above must not force it. The later call
                        // must reuse this acceptance.
                        let fd = cap.client.get_fd().await.unwrap().unwrap();
                        let file = std::fs::File::from(fd.try_clone().unwrap());
                        let mut bytes = [0; 10];
                        file.read_exact_at(&mut bytes, 0).unwrap();
                        assert_eq!(&bytes, b"handoff fd");
                    }
                    use_cap(&cap, id as u32).await;
                    expected_calls.push(id as u32);
                }
                used[id] = true;
            }
            "drop-b" => s.drop_cap(2),
            "drop-c" => s.drop_cap(3),
            "drop-d" => s.drop_cap(last),
            "drop-r" => {
                reflected.take();
            }
            other => panic!("unknown action {other}"),
        }
        drain().await;
        let hub = s.hub.borrow();
        let actual = vec![
            s.slots[2].borrow().is_some() as u64,
            s.slots[3].borrow().is_some() as u64,
            s.slots[last].borrow().is_some() as u64,
            reflected.is_some() as u64,
            used[0] as u64,
            used[1] as u64,
            used[2] as u64,
            used[3] as u64,
            forwards[0] as u64,
            forwards[1] as u64,
            hub.accept_count,
            s.calls.borrow().len() as u64,
            hub.provision_count() as u64,
            hub.local_accept_count,
        ];
        assert_eq!(
            actual, step.state,
            "trace {trace:?}, action {}",
            step.action
        );
        assert_eq!(*s.calls.borrow(), expected_calls);
        // The introducer's host connection is used only by this provision.
        // Once every deferred vine is gone, both ends must become idle.
        let provisions = hub.provision_count();
        drop(hub);
        let origin = s.hub.borrow_mut().connect(1, 0).status();
        let provider = s.hub.borrow_mut().connect(0, 1).status();
        assert_eq!(origin.borrow().idle, provisions == 0);
        assert_eq!(provider.borrow().idle, provisions == 0);
        let hub = s.hub.borrow();
        assert_eq!(
            hub.introduction_count, 1,
            "forwarding must reuse the provision"
        );
        assert_eq!(
            hub.forward_count,
            if trace.reject_forward {
                0
            } else {
                forwards.iter().filter(|v| **v).count() as u64
            }
        );
    }
    drop(s);
    drain().await;
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_deferred_handoff_traces() {
    let path = reproto_test_support::verification::input("REPROTO_DEFERRED_HANDOFF_TRACES")
        .expect("prepare verified trace corpus");
    let traces: Vec<Trace> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for trace in &traces {
                tokio::time::timeout(Duration::from_secs(5), replay(trace))
                    .await
                    .unwrap_or_else(|_| panic!("trace timed out: {trace:?}"));
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn forwarded_self_introduction_recovers_local_server_identity() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                let s = Setup::configured(true, true, false).await;
                drop(send(&s.bc, s.cap(2)).await);
                s.drop_cap(2);
                drop(send(&s.cd, s.cap(3)).await);
                s.drop_cap(3);
                use_cap(&s.cap(0), 123).await;
                let local = capnp::capability::get_resolved_cap(s.cap(0)).await;
                // Resolve to the actual original server, with no RPC proxy.
                assert_eq!(
                    local.into_client_hook().get_ptr(),
                    s.local_owner.clone().into_client_hook().get_ptr()
                );
                drain().await;
                assert_eq!(s.hub.borrow().local_accept_count, 1);
                assert_eq!(s.hub.borrow().accept_count, 1);
                assert_eq!(s.hub.borrow().provision_count(), 0);
            })
            .await
            .unwrap();
        })
        .await;
}

#[derive(serde::Deserialize, Debug)]
struct DisconnectTrace {
    host_link: bool,
    steps: Vec<Step>,
}
async fn replay_disconnect(trace: &DisconnectTrace) {
    let s = Setup::new(true).await;
    drop(send(&s.bc, s.cap(2)).await);
    s.drop_cap(2);
    drain().await;
    let mut connected = true;
    let mut used = false;
    let mut again = false;
    let mut accepted = false;
    let mut failed = false;
    for step in &trace.steps {
        match step.action.as_str() {
            "disconnect" => {
                s.hub
                    .borrow_mut()
                    .disconnect_pair(1, if trace.host_link { 0 } else { 2 });
                connected = false;
            }
            "use" | "again" => {
                let mut request = s.cap(3).echo_request();
                request.get().set_value(7);
                let result = request.send().promise.await;
                if step.action == "use" {
                    used = true;
                    accepted = result.is_ok();
                    failed = result.is_err();
                } else {
                    again = true;
                    assert_eq!(result.is_ok(), accepted);
                }
            }
            "drop" => s.drop_cap(3),
            other => panic!("unknown action {other}"),
        }
        drain().await;
        // Unrelated authority on the downstream connection must survive a lost vine.
        use_cap(&s.bc, 999).await;
        let actual = vec![
            connected as u64,
            s.slots[3].borrow().is_some() as u64,
            used as u64,
            again as u64,
            accepted as u64,
            failed as u64,
            s.calls.borrow().iter().filter(|v| **v == 7).count() as u64,
            s.hub.borrow().provision_count() as u64,
            1,
        ];
        assert_eq!(actual, step.state, "{trace:?} action {}", step.action);
    }
    drop(s);
    drain().await;
}
#[tokio::test(flavor = "current_thread")]
async fn lost_forwarded_vine_does_not_abort_downstream_connection() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for host_link in [false, true] {
                let trace = DisconnectTrace {
                    host_link,
                    steps: vec![
                        Step {
                            action: "disconnect".into(),
                            state: vec![0, 1, 0, 0, 0, 0, 0, 0, 1],
                        },
                        Step {
                            action: "use".into(),
                            state: vec![0, 1, 1, 0, 0, 1, 0, 0, 1],
                        },
                    ],
                };
                tokio::time::timeout(Duration::from_secs(5), replay_disconnect(&trace))
                    .await
                    .unwrap();
            }
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_deferred_disconnect_traces() {
    let path = reproto_test_support::verification::input("REPROTO_DEFERRED_DISCONNECT_TRACES")
        .expect("prepare verified trace corpus");
    let traces: Vec<DisconnectTrace> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for trace in &traces {
                tokio::time::timeout(Duration::from_secs(5), replay_disconnect(trace))
                    .await
                    .unwrap_or_else(|_| panic!("trace timed out: {trace:?}"));
            }
        })
        .await;
}
