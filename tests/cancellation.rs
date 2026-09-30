mod support;
use capnp::traits::HasTypeId;
use capnp_rpc::{
    rpc_capnp::{message, return_},
    RpcSystem,
};
use futures::channel::oneshot;
use reproto_test_support::runtime_test_capnp::harness;
use std::{cell::RefCell, rc::Rc, time::Duration};
use support::{Endpoint, Hub};

#[derive(Default)]
struct Control {
    receiver: Option<oneshot::Receiver<()>>,
    held: Option<harness::PendingResults>,
    started: u32,
    completed: u32,
    dropped: u32,
    echoes: u32,
}
struct Running(Rc<RefCell<Control>>);
impl Drop for Running {
    fn drop(&mut self) {
        self.0.borrow_mut().dropped += 1;
    }
}
struct Service(Rc<RefCell<Control>>);
impl harness::Server for Service {
    async fn echo(
        self: Rc<Self>,
        _: harness::EchoParams,
        mut r: harness::EchoResults,
    ) -> capnp::Result<()> {
        self.0.borrow_mut().echoes += 1;
        r.get().set_value(7);
        Ok(())
    }
    async fn pending(
        self: Rc<Self>,
        p: harness::PendingParams,
        r: harness::PendingResults,
    ) -> capnp::Result<()> {
        let _running = Running(self.0.clone());
        self.0.borrow_mut().started += 1;
        let mut r = if p.get()?.get_hold_context() {
            self.0.borrow_mut().held = Some(r);
            None
        } else {
            Some(r)
        };
        let receiver = self.0.borrow_mut().receiver.take().unwrap();
        receiver
            .await
            .map_err(|_| capnp::Error::failed("gate failed".into()))?;
        let mut results = r
            .take()
            .or_else(|| self.0.borrow_mut().held.take())
            .unwrap();
        let cap: harness::Client = capnp_rpc::new_client_from_rc(self.clone());
        results.get().set_cap(cap);
        self.0.borrow_mut().completed += 1;
        Ok(())
    }
}
async fn settle() {
    for _ in 0..32 {
        tokio::task::yield_now().await;
    }
}
async fn receive_return(peer: &mut Endpoint) -> (u32, &'static str) {
    let input = tokio::time::timeout(Duration::from_secs(1), peer.recv())
        .await
        .expect("expected Return, got no message");
    let root: message::Reader = input.get_body().unwrap().get_as().unwrap();
    let message::Return(ret) = root.which().unwrap() else {
        panic!("expected Return, got a different message")
    };
    let ret = ret.unwrap();
    (
        ret.get_answer_id(),
        match ret.which().unwrap() {
            return_::Results(_) => "results",
            return_::Canceled(()) => "canceled",
            return_::Exception(_) => "exception",
            return_::ResultsSentElsewhere(()) => "elsewhere",
            _ => panic!("unexpected return type"),
        },
    )
}
fn pending(peer: &mut Endpoint, id: u32, export: u32, hold: bool, redirect: bool) {
    peer.send(|m| {
        let mut c = m.init_call();
        c.set_question_id(id);
        c.set_interface_id(harness::Client::TYPE_ID);
        c.set_method_id(3);
        c.reborrow().init_target().set_imported_cap(export);
        if redirect {
            c.reborrow().init_send_results_to().set_yourself(());
        }
        c.init_params()
            .get_content()
            .init_as::<harness::pending_params::Builder>()
            .set_hold_context(hold);
    });
}
fn finish(peer: &mut Endpoint, id: u32) {
    peer.send(|m| {
        let mut f = m.init_finish();
        f.set_question_id(id);
        f.set_require_early_cancellation_workaround(false);
    });
}
async fn fixture() -> (
    Rc<RefCell<Control>>,
    oneshot::Sender<()>,
    Endpoint,
    u32,
    tokio::task::JoinHandle<capnp::Result<()>>,
) {
    let control = Rc::new(RefCell::new(Control::default()));
    let (tx, rx) = oneshot::channel();
    control.borrow_mut().receiver = Some(rx);
    let hub = Rc::new(RefCell::new(Hub::default()));
    let network = Hub::network(&hub, 1);
    assert_eq!(hub.borrow().provision_count(), 0);
    let service: harness::Client = capnp_rpc::new_client(Service(control.clone()));
    let task = tokio::task::spawn_local(RpcSystem::new(Box::new(network), Some(service.client)));
    let mut peer = hub.borrow_mut().connect(2, 1);
    peer.send(|m| m.init_bootstrap().set_question_id(0));
    let reply = peer.recv().await;
    let message::Return(ret) = reply
        .get_body()
        .unwrap()
        .get_as::<message::Reader>()
        .unwrap()
        .which()
        .unwrap()
    else {
        panic!()
    };
    let return_::Results(payload) = ret.unwrap().which().unwrap() else {
        panic!()
    };
    let capnp_rpc::rpc_capnp::cap_descriptor::SenderHosted(export) = payload
        .unwrap()
        .get_cap_table()
        .unwrap()
        .get(0)
        .which()
        .unwrap()
    else {
        panic!()
    };
    (control, tx, peer, export, task)
}
#[tokio::test(flavor = "current_thread")]
async fn canceled_call_returns_once_and_releases_answer_id() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (control, _gate, mut peer, export, task) = fixture().await;
            pending(&mut peer, 1, export, false, false);
            settle().await;
            assert_eq!(control.borrow().started, 1);
            finish(&mut peer, 1);
            settle().await;
            assert_eq!(control.borrow().dropped, 1);
            assert_eq!(receive_return(&mut peer).await, (1, "canceled"));
            // Reuse the ID only after Finish AND Return, as specified.
            peer.send(|m| m.init_bootstrap().set_question_id(1));
            assert_eq!(receive_return(&mut peer).await, (1, "results"));
            task.abort();
            let _ = task.await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn legacy_early_finish_allows_dispatch_before_cancellation() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (control, _gate, mut peer, export, task) = fixture().await;
            pending(&mut peer, 1, export, false, false);
            peer.send(|m| {
                let mut f = m.init_finish();
                f.set_question_id(1);
                f.set_require_early_cancellation_workaround(true);
            });
            settle().await;
            assert_eq!(control.borrow().started, 1);
            assert_eq!(control.borrow().dropped, 1);
            assert_eq!(receive_return(&mut peer).await, (1, "canceled"));
            task.abort();
            let _ = task.await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn retained_context_delays_cancellation_return_and_survives_disconnect() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for disconnect in [false, true] {
                let (control, _gate, mut peer, export, mut task) = fixture().await;
                pending(&mut peer, 1, export, true, false);
                settle().await;
                finish(&mut peer, 1);
                settle().await;
                assert_eq!(control.borrow().dropped, 1);
                assert!(tokio::time::timeout(Duration::from_millis(2), peer.recv())
                    .await
                    .is_err());
                if disconnect {
                    task.abort();
                    let _ = (&mut task).await;
                }
                let held = control.borrow_mut().held.take();
                drop(held);
                settle().await;
                if !disconnect {
                    assert_eq!(receive_return(&mut peer).await, (1, "canceled"));
                    task.abort();
                    let _ = task.await;
                }
            }
        })
        .await;
}
fn pipelined_echo(peer: &mut Endpoint, question: u32, parent: u32) {
    peer.send(|m| {
        let mut c = m.init_call();
        c.set_question_id(question);
        c.set_interface_id(harness::Client::TYPE_ID);
        c.set_method_id(0);
        let mut t = c.reborrow().init_target().init_promised_answer();
        t.set_question_id(parent);
        t.init_transform(1).get(0).set_get_pointer_field(0);
        c.init_params()
            .get_content()
            .init_as::<harness::echo_params::Builder>()
            .set_value(3);
    });
}
#[tokio::test(flavor = "current_thread")]
async fn child_pipeline_keeps_parent_running_after_finish() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (control, gate, mut peer, export, task) = fixture().await;
            pending(&mut peer, 1, export, false, false);
            settle().await;
            pipelined_echo(&mut peer, 2, 1);
            settle().await;
            finish(&mut peer, 1);
            settle().await;
            assert_eq!(control.borrow().dropped, 0);
            gate.send(()).unwrap();
            settle().await;
            let mut returns = vec![
                receive_return(&mut peer).await,
                receive_return(&mut peer).await,
            ];
            returns.sort();
            assert_eq!(returns, vec![(1, "canceled"), (2, "results")]);
            assert_eq!(control.borrow().completed, 1);
            assert_eq!(control.borrow().echoes, 1);
            task.abort();
            let _ = task.await;
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn redirected_cancellation_and_completion_return_elsewhere_once() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for complete in [false, true] {
                let (control, gate, mut peer, export, task) = fixture().await;
                pending(&mut peer, 1, export, false, true);
                settle().await;
                if complete {
                    gate.send(()).unwrap();
                    settle().await;
                }
                finish(&mut peer, 1);
                settle().await;
                assert_eq!(receive_return(&mut peer).await, (1, "elsewhere"));
                assert_eq!(control.borrow().completed, u32::from(complete));
                peer.send(|m| m.init_bootstrap().set_question_id(1));
                assert_eq!(receive_return(&mut peer).await, (1, "results"));
                task.abort();
                let _ = task.await;
            }
        })
        .await;
}

// A raw peer asks us to execute its tail call, then redirects our outstanding
// call to that result. No second RPC engine can accidentally repair ownership.
async fn adoption_fixture() -> (
    Rc<RefCell<Control>>,
    oneshot::Sender<()>,
    Endpoint,
    u32,
    capnp::capability::RemotePromise<harness::pending_results::Owned>,
    tokio::task::JoinHandle<capnp::Result<()>>,
) {
    let control = Rc::new(RefCell::new(Control::default()));
    let (gate, rx) = oneshot::channel();
    control.borrow_mut().receiver = Some(rx);
    let hub = Rc::new(RefCell::new(Hub::default()));
    let network = Hub::network(&hub, 1);
    let service: harness::Client = capnp_rpc::new_client(Service(control.clone()));
    let mut system = RpcSystem::new(Box::new(network), Some(service.client));
    let mut peer = hub.borrow_mut().connect(2, 1);
    let remote: harness::Client = system.bootstrap(2);
    let request = remote.pending_request().send();
    let task = tokio::task::spawn_local(system);
    // Collect the outgoing pipelined call ID. Its bootstrap can remain pending.
    let outgoing_id = loop {
        let input = peer.recv().await;
        if let message::Call(c) = input
            .get_body()
            .unwrap()
            .get_as::<message::Reader>()
            .unwrap()
            .which()
            .unwrap()
        {
            break c.unwrap().get_question_id();
        }
    };
    peer.send(|m| m.init_bootstrap().set_question_id(0));
    let export = loop {
        let input = peer.recv().await;
        let message::Return(r) = input
            .get_body()
            .unwrap()
            .get_as::<message::Reader>()
            .unwrap()
            .which()
            .unwrap()
        else {
            continue;
        };
        let return_::Results(p) = r.unwrap().which().unwrap() else {
            panic!();
        };
        let capnp_rpc::rpc_capnp::cap_descriptor::SenderHosted(id) =
            p.unwrap().get_cap_table().unwrap().get(0).which().unwrap()
        else {
            panic!();
        };
        break id;
    };
    pending(&mut peer, 1, export, false, true);
    settle().await;
    assert_eq!(control.borrow().started, 1);
    (control, gate, peer, outgoing_id, request, task)
}
fn adopt(peer: &mut Endpoint, outgoing_id: u32) {
    peer.send(|m| {
        let mut ret = m.init_return();
        ret.set_answer_id(outgoing_id);
        ret.set_take_from_other_question(1);
    });
}
#[tokio::test(flavor = "current_thread")]
async fn adopted_redirected_call_survives_finish_until_completion() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (control, gate, mut peer, outgoing_id, request, task) = adoption_fixture().await;
            adopt(&mut peer, outgoing_id);
            settle().await;
            finish(&mut peer, 1);
            settle().await;
            assert_eq!(
                control.borrow().dropped,
                0,
                "Finish canceled an adopted task"
            );
            gate.send(()).unwrap();
            let response = tokio::time::timeout(Duration::from_secs(1), request.promise)
                .await
                .unwrap()
                .unwrap();
            let cap = response.get().unwrap().get_cap().unwrap();
            assert_eq!(
                cap.echo_request()
                    .send()
                    .promise
                    .await
                    .unwrap()
                    .get()
                    .unwrap()
                    .get_value(),
                7
            );
            assert_eq!(control.borrow().completed, 1);
            task.abort();
            let _ = task.await;
        })
        .await;
}

#[derive(serde::Deserialize, Debug)]
struct Step {
    action: String,
    state: [u32; 11],
}
#[derive(serde::Deserialize, Debug)]
struct Trace {
    hold: bool,
    redirect: bool,
    steps: Vec<Step>,
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_cancellation_traces() {
    use capnp_rpc::Connection;
    use futures::FutureExt;
    let path = reproto_test_support::verification::input("REPROTO_CANCELLATION_TRACES")
        .expect("prepare verified trace corpus");
    let traces: Vec<Trace> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(!traces.is_empty());
    tokio::task::LocalSet::new().run_until(async {
        tokio::time::timeout(Duration::from_secs(60),async {
            for trace in traces {
                let (control,gate,mut peer,export,mut task) = fixture().await;
                let mut gate = Some(gate);
                pending(&mut peer,1,export,trace.hold,trace.redirect); settle().await;
                let mut parent_returns = vec![];
                let mut child_returns = vec![];
                let mut connected = true;
                for (index,step) in trace.steps.iter().enumerate() {
                    match step.action.as_str() {
                        "finish" | "repeat_finish" => finish(&mut peer,1),
                        "pipeline" => pipelined_echo(&mut peer,2,1),
                        "cancel_child" => finish(&mut peer,2),
                        "complete" => { assert!(gate.take().unwrap().send(()).is_ok(),"canceled task in {:?}",trace); },
                        "drop_context" => { let held = control.borrow_mut().held.take(); drop(held); },
                        "disconnect" => { task.abort(); let _ = (&mut task).await; connected=false; },
                        "reuse" => { peer.send(|m| m.init_bootstrap().set_question_id(1)); settle().await;
                            assert_eq!(receive_return(&mut peer).await,(1,"results"),"reused ID in {:?}",trace);
                            finish(&mut peer,1);
                        },
                        other => panic!("unknown TLC action {other}"),
                    }
                    settle().await;
                    loop {
                        let Some(input) = peer.receive_incoming_message().now_or_never() else { break; };
                        let Some(input) = input.unwrap() else { break; };
                        let root: message::Reader = input.get_body().unwrap().get_as().unwrap();
                        match root.which().unwrap() {
                            message::Abort(_) => assert!(!connected,"unexpected Abort in {:?}",trace),
                            message::Return(ret) => {
                                let ret=ret.unwrap(); let kind=match ret.which().unwrap() {
                                    return_::Results(_) => 1, return_::Canceled(()) => 2,
                                    return_::ResultsSentElsewhere(()) => 3,
                                    _ => panic!("unexpected Return in {:?}",trace),
                                };
                                match ret.get_answer_id() {
                                    1 => parent_returns.push(kind),2 => child_returns.push(kind),
                                    id => panic!("unexpected answer {id} in {:?}",trace),
                                }
                            },
                            _ => panic!("unexpected wire message in {:?}",trace),
                        }
                    }
                    let [_,held,_,completed,running,_,_,result,returns,_,child_result] = step.state;
                    let context = format!("prefix {:?}",&trace.steps[..=index]);
                    assert_eq!(parent_returns.len(),returns as usize,"{context}");
                    if result!=0 { assert_eq!(parent_returns,vec![result],"{context}"); }
                    let expected_child=match child_result { 0=>vec![],1=>vec![1],2=>vec![2],_=>unreachable!() };
                    assert_eq!(child_returns,expected_child,"{context}");
                    let ctl=control.borrow();
                    assert_eq!(ctl.completed,completed,"{context}");
                    assert_eq!(ctl.dropped,1-running,"{context}");
                    assert_eq!(ctl.echoes,u32::from(child_result==1),"{context}");
                    assert_eq!(u32::from(ctl.held.is_some()),held,"{context}");
                }
                if connected { task.abort(); let _ = task.await; }
                let held=control.borrow_mut().held.take(); drop(held);
            }
        }).await.unwrap();
    }).await;
}

#[derive(serde::Deserialize, Debug)]
struct AdoptionStep {
    action: String,
    state: [u32; 7],
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_tail_adoption_traces() {
    use capnp_rpc::Connection;
    use futures::FutureExt;
    let path = reproto_test_support::verification::input("REPROTO_TAIL_ADOPTION_TRACES")
        .expect("prepare verified trace corpus");
    let traces: Vec<Vec<AdoptionStep>> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(!traces.is_empty());
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(60), async {
                for trace in traces {
                    let (control, gate, mut peer, outgoing_id, request, task) =
                        adoption_fixture().await;
                    let mut request = Some(request);
                    let mut gate = Some(gate);
                    let mut returns = 0;
                    for step in &trace {
                        match step.action.as_str() {
                            "adopt" => adopt(&mut peer, outgoing_id),
                            "finish" => finish(&mut peer, 1),
                            "cancel" => drop(request.take()),
                            "complete" => {
                                assert!(gate.take().unwrap().send(()).is_ok(), "{trace:?}");
                            }
                            "use" => {
                                let response = request.take().unwrap().promise.await.unwrap();
                                let cap = response.get().unwrap().get_cap().unwrap();
                                assert_eq!(
                                    cap.echo_request()
                                        .send()
                                        .promise
                                        .await
                                        .unwrap()
                                        .get()
                                        .unwrap()
                                        .get_value(),
                                    7
                                );
                            }
                            other => panic!("unknown action {other}"),
                        }
                        settle().await;
                        loop {
                            let Some(input) = peer.receive_incoming_message().now_or_never() else {
                                break;
                            };
                            let input = input.unwrap().unwrap();
                            match input
                                .get_body()
                                .unwrap()
                                .get_as::<message::Reader>()
                                .unwrap()
                                .which()
                                .unwrap()
                            {
                                message::Return(ret) => {
                                    let ret = ret.unwrap();
                                    assert_eq!(ret.get_answer_id(), 1, "{trace:?}");
                                    assert!(
                                        matches!(
                                            ret.which().unwrap(),
                                            return_::ResultsSentElsewhere(())
                                        ),
                                        "{trace:?}"
                                    );
                                    returns += 1;
                                }
                                message::Abort(_) => panic!("connection aborted in {trace:?}"),
                                _ => {} // Outgoing question Finish and capability Release.
                            }
                        }
                        assert_eq!(
                            returns, step.state[5],
                            "Return count at {step:?} in {trace:?}"
                        );
                        let c = control.borrow();
                        assert_eq!(c.started, 1);
                        assert_eq!(
                            c.completed, step.state[3],
                            "completion at {step:?} in {trace:?}"
                        );
                        assert_eq!(
                            c.dropped,
                            1 - step.state[4],
                            "task lifetime at {step:?} in {trace:?}"
                        );
                        assert_eq!(
                            c.echoes, step.state[6],
                            "capability use at {step:?} in {trace:?}"
                        );
                    }
                    drop(request);
                    task.abort();
                    let _ = task.await;
                }
            })
            .await
            .expect("tail adoption replay timed out");
        })
        .await;
}
