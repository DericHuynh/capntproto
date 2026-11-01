//! Grant-matcher and broken-reference regressions from the E equality research.
use super::*;
use futures::{channel::oneshot, FutureExt};

struct Destination(Rc<Cell<u32>>);
impl harness::Server for Destination {
    fn echo(
        self: Rc<Self>,
        context: harness::EchoContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let (_, _) = context.into_owned();
        capnp::capability::Promise::from_future(async move {
            self.0.set(self.0.get() + 1);
            Ok(())
        })
    }
}

// An ordinary application forwarder may selectively redirect calls. Its
// apparent behavior must never make it identical to the inner capability.
struct Forwarder(harness::Client, Rc<Cell<u32>>);
impl harness::Server for Forwarder {
    fn echo(
        self: Rc<Self>,
        context: harness::EchoContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let (p, r) = context.into_owned();
        capnp::capability::Promise::from_future(async move {
            if p.get()?.get_value() == 20 {
                self.1.set(self.1.get() + 1);
                Ok(())
            } else {
                r.hook.tail_call(self.0.echo_request().hook).await
            }
        })
    }
}

fn system() -> RpcSystem {
    let hub = Rc::new(RefCell::new(support::Hub::default()));
    RpcSystem::new(Box::new(support::Hub::network(&hub, 13)), None)
}

#[tokio::test(flavor = "current_thread")]
async fn grant_matcher_does_not_confuse_a_selective_forwarder_with_its_target() {
    for _ in [()] {
        for reverse in [false, true] {
            let system = system();
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
    for _ in [()] {
        for outcome in 0..3 {
            let system = system();
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
    for _ in [()] {
        let system = system();
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
    for _ in [()] {
        let system = system();
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
    for _ in [()] {
        let system = system();
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
    for _ in [()] {
        let system = system();
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
