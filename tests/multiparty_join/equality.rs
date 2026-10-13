//! Grant-matcher and broken-reference regressions from the E equality research.
use super::*;
use futures::{channel::oneshot, FutureExt};

struct Destination(Rc<Cell<u32>>);
impl harness::Server for Destination {
    async fn echo(
        self: Rc<Self>,
        _: harness::EchoParams,
        _: harness::EchoResults,
    ) -> capnp::Result<()> {
        self.0.set(self.0.get() + 1);
        Ok(())
    }
}

// An ordinary application forwarder may selectively redirect calls. Its
// apparent behavior must never make it identical to the inner capability.
struct Forwarder(harness::Client, Rc<Cell<u32>>);
impl harness::Server for Forwarder {
    async fn echo(
        self: Rc<Self>,
        p: harness::EchoParams,
        r: harness::EchoResults,
    ) -> capnp::Result<()> {
        if p.get()?.get_value() == 20 {
            self.1.set(self.1.get() + 1);
            Ok(())
        } else {
            r.hook.tail_call(self.0.echo_request().hook).await
        }
    }
}

fn system(portable: bool) -> RpcSystem {
    let (network, _) = Network::new([13; 32]);
    RpcSystem::new(portable, Box::new(network), None)
}

#[tokio::test(flavor = "current_thread")]
async fn grant_matcher_does_not_confuse_a_selective_forwarder_with_its_target() {
    for portable in [false, true] {
        for reverse in [false, true] {
            let system = system(portable);
            let joiner = system.get_joiner();
            let donated = Rc::new(Cell::new(0));
            let stolen = Rc::new(Cell::new(0));
            let honest: harness::Client = capnp_rpc::new_client(Destination(donated.clone()));
            let forwarder: harness::Client =
                capnp_rpc::new_client(Forwarder(honest.clone(), stolen.clone()));
            forwarder.echo_request().send().promise.await.unwrap();
            assert_eq!(donated.get(), 1);
            let mut inputs = vec![honest.clone(), forwarder];
            if reverse {
                inputs.reverse();
            }
            assert!(joiner.join(inputs).await.unwrap().is_none());
            assert_eq!(stolen.get(), 0);
            let joined = joiner
                .join(vec![honest.clone(), honest])
                .await
                .unwrap()
                .unwrap();
            let mut gift = joined.echo_request();
            gift.get().set_value(20);
            gift.send().promise.await.unwrap();
            assert_eq!(donated.get(), 2);
            assert_eq!(stolen.get(), 0);
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn unsettled_join_never_guesses_inequality_or_delivers_failed_pipelined_calls() {
    for portable in [false, true] {
        for outcome in 0..3 {
            let system = system(portable);
            let joiner = system.get_joiner();
            let delivered = Rc::new(Cell::new(0));
            let honest: harness::Client = capnp_rpc::new_client(Destination(delivered.clone()));
            let (tx, rx) = oneshot::channel::<capnp::Result<harness::Client>>();
            let pending = capnp_rpc::new_future_client(async move {
                rx.await
                    .map_err(|_| Error::disconnected("resolution lost".into()))?
            });
            let mut joining = joiner.join(vec![honest.clone(), pending]);
            assert!((&mut joining).now_or_never().is_none());
            let joined: harness::Client = capnp_rpc::new_future_client(async move {
                joining
                    .await?
                    .ok_or_else(|| Error::failed("unequal destinations".into()))
            });
            let mut call = joined.echo_request().send().promise;
            assert!((&mut call).now_or_never().is_none());
            assert_eq!(delivered.get(), 0);
            let resolution = match outcome {
                0 => Ok(honest),
                1 => Ok(capnp_rpc::new_client(Destination(delivered.clone()))),
                _ => Err(Error::disconnected("resolution lost".into())),
            };
            assert!(tx.send(resolution).is_ok());
            assert_eq!(call.await.is_ok(), outcome == 0);
            assert_eq!(delivered.get(), u32::from(outcome == 0));
            // Broken joins remain broken when called again.
            assert_eq!(
                joined.echo_request().send().promise.await.is_ok(),
                outcome == 0
            );
            assert_eq!(delivered.get(), 2 * u32::from(outcome == 0));
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn join_rejects_revoked_inputs_instead_of_proving_equality_or_inequality() {
    for portable in [false, true] {
        let system = system(portable);
        let joiner = system.get_joiner();
        let delivered = Rc::new(Cell::new(0));
        let owner =
            capnp_rpc::RevocableServer::<harness::Client>::new(Destination(delivered.clone()));
        let dead = owner.get_client();
        owner.revoke_with_error(Error::failed("destination revoked".into()));
        let live: harness::Client = capnp_rpc::new_client(Destination(delivered.clone()));
        for inputs in [
            vec![dead.clone(), dead.clone()],
            vec![dead.clone(), live.clone()],
            vec![live, dead],
        ] {
            let result = joiner.join(inputs).await;
            let error = result
                .err()
                .expect("broken input is an error, never an equality result");
            assert_eq!(error.extra, "destination revoked");
        }
        assert_eq!(delivered.get(), 0);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn join_rechecks_earlier_inputs_after_waiting_for_later_resolution() {
    for portable in [false, true] {
        let system = system(portable);
        let joiner = system.get_joiner();
        let owner = capnp_rpc::RevocableServer::<harness::Client>::new(Destination(Rc::default()));
        let (tx, rx) = oneshot::channel::<harness::Client>();
        let later = capnp_rpc::new_future_client(async move {
            rx.await
                .map_err(|_| Error::disconnected("resolution lost".into()))
        });
        let mut joining = joiner.join(vec![owner.get_client(), later]);
        assert!((&mut joining).now_or_never().is_none());
        owner.revoke_with_error(Error::failed("revoked while joining".into()));
        assert!(tx
            .send(capnp_rpc::new_client(Destination(Rc::default())))
            .is_ok());
        assert_eq!(joining.await.err().unwrap().extra, "revoked while joining");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn settled_broken_capabilities_do_not_become_valid_join_endpoints() {
    for portable in [false, true] {
        let system = system(portable);
        let joiner = system.get_joiner();
        let server = Rc::new(Destination(Rc::default()));
        let owner = capnp_rpc::RevocableServer::<harness::Client>::from_rc(server.clone());
        // Issuing an unrevocable alias to the same server is forbidden. The
        // returned broken hook is settled, unlike a rejected promise.
        let forbidden: harness::Client = capnp_rpc::new_client_from_rc(server);
        assert!(forbidden.client.hook.when_more_resolved().is_none());
        assert!(joiner
            .join(vec![forbidden.clone(), forbidden.clone()])
            .await
            .is_err());
        let membrane = capnp_rpc::membrane::Membrane::new(Rc::new(JoinBoundary {
            allowed: true,
            joins: Cell::new(0),
            calls: Cell::new(0),
            blocked: Cell::new(false),
        }));
        let wrapped = membrane.export(forbidden);
        assert!(joiner.join(vec![wrapped.clone(), wrapped]).await.is_err());
        assert!(joiner
            .join(vec![owner.get_client(), owner.get_client()])
            .await
            .unwrap()
            .is_some());
    }
}

struct Shortened(RefCell<Option<harness::Client>>);
impl capnp::capability::ServerHooks for Shortened {
    fn shorten_path(&self) -> Option<Promise<capnp::capability::Client, Error>> {
        self.0
            .borrow_mut()
            .take()
            .map(|cap| Promise::ok(cap.client))
    }
}
impl harness::Server for Shortened {
    fn _capnp_server_hooks(&self) -> Option<&dyn capnp::capability::ServerHooks> {
        Some(self)
    }
}

#[tokio::test(flavor = "current_thread")]
async fn joining_shortened_authority_does_not_depend_on_its_former_owner() {
    for portable in [false, true] {
        let system = system(portable);
        let joiner = system.get_joiner();
        let delivered = Rc::new(Cell::new(0));
        let target: harness::Client = capnp_rpc::new_client(Destination(delivered.clone()));
        let owner = capnp_rpc::RevocableServer::<harness::Client>::new(Shortened(RefCell::new(
            Some(target.clone()),
        )));
        let original = owner.get_client();
        original.client.hook.when_resolved().await.unwrap();
        owner.revoke();
        let joined = joiner.join(vec![original, target]).await.unwrap().unwrap();
        joined.echo_request().send().promise.await.unwrap();
        assert_eq!(delivered.get(), 1);
    }
}

struct Ordered(Rc<RefCell<Vec<u32>>>);
impl harness::Server for Ordered {
    async fn echo(
        self: Rc<Self>,
        p: harness::EchoParams,
        mut r: harness::EchoResults,
    ) -> capnp::Result<()> {
        let value = p.get()?.get_value();
        self.0.borrow_mut().push(value);
        r.get().set_value(value);
        Ok(())
    }
}

fn send(
    cap: &harness::Client,
    value: u32,
) -> Promise<capnp::capability::Response<harness::value::Owned>, Error> {
    let mut request = cap.echo_request();
    request.get().set_value(value);
    request.send().promise
}

#[tokio::test(flavor = "current_thread")]
async fn four_party_join_waits_for_both_histories_and_never_delivers_on_failure() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(30), async {
                for portable in [false, true] {
                    for outcome in 0..4 {
                        let ids: Vec<_> = (0..4).map(|_| Identity::generate()).collect();
                        let pairs: Vec<_> =
                            ids.iter().map(|id| Network::new(id.public_key())).collect();
                        let (mut networks, handles): (Vec<_>, Vec<_>) = pairs.into_iter().unzip();
                        // A receives independent references through B and C. D hosts
                        // the endpoint. Its direct route to A is used only by Join.
                        for (i, j) in [(0, 1), (0, 2), (1, 3), (2, 3), (0, 3)] {
                            pair(&ids[i], &ids[j], &handles[i], &handles[j]).await;
                        }
                        let log = Rc::new(RefCell::new(Vec::new()));
                        let first: harness::Client = capnp_rpc::new_client(Ordered(log.clone()));
                        let second = if outcome == 1 {
                            capnp_rpc::new_client(Ordered(log.clone()))
                        } else {
                            first.clone()
                        };
                        let d = RpcSystem::new_with_bootstrap_factory(
                            portable,
                            Box::new(Proxies(networks.pop().unwrap(), true)),
                            ids[3].public_key(),
                            Rc::new(Host {
                                other: ids[2].public_key(),
                                first,
                                second,
                            }),
                        );
                        let cf = Rc::new(Relay(RefCell::new(None)));
                        let mut c = RpcSystem::new_with_bootstrap_factory(
                            portable,
                            Box::new(Proxies(networks.pop().unwrap(), true)),
                            ids[2].public_key(),
                            cf.clone(),
                        );
                        let target: harness::Client = c.bootstrap(ids[3].public_key());
                        let (tx, rx) = oneshot::channel::<()>();
                        *cf.0.borrow_mut() = Some(capnp_rpc::new_future_client(async move {
                            rx.await
                                .map_err(|_| Error::disconnected("right path lost".into()))?;
                            Ok(target)
                        }));
                        let bf = Rc::new(Relay(RefCell::new(None)));
                        let mut b = RpcSystem::new_with_bootstrap_factory(
                            portable,
                            Box::new(Proxies(networks.pop().unwrap(), true)),
                            ids[1].public_key(),
                            bf.clone(),
                        );
                        *bf.0.borrow_mut() = Some(b.bootstrap(ids[3].public_key()));
                        let mut a = RpcSystem::new(
                            portable,
                            Box::new(Proxies(networks.pop().unwrap(), true)),
                            None,
                        );
                        let joiner = a.get_joiner();
                        let left: harness::Client = a.bootstrap(ids[1].public_key());
                        let right: harness::Client = a.bootstrap(ids[2].public_key());
                        let tasks = [a, b, c, d].map(|s| tokio::task::spawn_local(s.run()));
                        let u = send(&left, 1);
                        let mut v = send(&right, 2);
                        u.await.unwrap();
                        assert!((&mut v).now_or_never().is_none());
                        let joined: harness::Client = capnp_rpc::new_future_client(async move {
                            joiner
                                .join(vec![left, right])
                                .await?
                                .ok_or_else(|| Error::failed("unequal destinations".into()))
                        });
                        let mut x = send(&joined, 3);
                        assert!((&mut x).now_or_never().is_none());
                        drain().await;
                        assert_eq!(*log.borrow(), vec![1]);
                        assert_eq!(handles[3].join_stats().accepted, 0);
                        if outcome == 3 {
                            // Lose C's already-established route to D while v
                            // is queued. Releasing the promise cannot revive it.
                            handles[2].disconnect(ids[3].public_key());
                            drain().await;
                        }
                        if outcome == 2 {
                            drop(tx);
                        } else {
                            tx.send(()).unwrap();
                        }
                        assert_eq!(v.await.is_ok(), outcome < 2);
                        assert_eq!(x.await.is_ok(), outcome == 0);
                        assert_eq!(
                            *log.borrow(),
                            match outcome {
                                0 => vec![1, 2, 3],
                                1 => vec![1, 2],
                                _ => vec![1],
                            },
                            "portable={portable}, outcome={outcome}"
                        );
                        assert_eq!(handles[3].join_stats().accepted, u64::from(outcome == 0));
                        for task in tasks {
                            task.abort();
                        }
                        drain().await;
                    }
                }
            })
            .await
            .unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn disconnected_import_aliases_cannot_report_a_successful_join() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(10), async {
                for portable in [false, true] {
                    let caller = Identity::generate();
                    let host = Identity::generate();
                    let (a, ah) = Network::new(caller.public_key());
                    let (b, bh) = Network::new(host.public_key());
                    pair(&caller, &host, &ah, &bh).await;
                    let mut a = RpcSystem::new(portable, Box::new(Proxies(a, true)), None);
                    let joiner = a.get_joiner();
                    let cap: harness::Client = a.bootstrap(host.public_key());
                    let b = RpcSystem::new(
                        portable,
                        Box::new(Proxies(b, true)),
                        Some(capnp_rpc::new_client::<harness::Client, _>(Echo(73)).client),
                    );
                    let tasks = [a, b].map(|s| tokio::task::spawn_local(s.run()));
                    let cap = capnp::capability::get_resolved_cap(cap).await;
                    assert!(cap.echo_request().send().promise.await.is_ok());
                    ah.disconnect(host.public_key());
                    assert!(cap.echo_request().send().promise.await.is_err());
                    assert!(joiner.join(vec![cap.clone(), cap.clone()]).await.is_err());
                    // A Join cannot reactivate the reference either.
                    assert!(cap.echo_request().send().promise.await.is_err());
                    for task in tasks {
                        task.abort();
                    }
                    drain().await;
                }
            })
            .await
            .unwrap();
        })
        .await;
}
