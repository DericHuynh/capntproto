//! Owned, compare-and-replace renewal. A task never recreates a revoked name.
use super::*;
use std::cell::Cell;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublicationStatus {
    Active {
        generation: DiscoveryGeneration,
        expires: Instant,
    },
    Revoked,
    Superseded,
    Expired,
    Closed,
    Exhausted,
    Stopped,
}

struct Lease {
    directory: Directory,
    key: (String, [u8; 32]),
    generation: Cell<DiscoveryGeneration>,
    lifetime: Duration,
    terminal: Cell<Option<PublicationStatus>>,
}
impl Lease {
    fn status(&self, now: Instant) -> PublicationStatus {
        if let Some(status) = self.terminal.get() {
            return status;
        }
        let state = self.directory.0.borrow();
        if state.closed {
            return PublicationStatus::Closed;
        }
        match state.entries.get(&self.key) {
            None => PublicationStatus::Revoked,
            Some(entry) if entry.generation != self.generation.get() => {
                PublicationStatus::Superseded
            }
            Some(entry) if now >= entry.expires => PublicationStatus::Expired,
            Some(entry) => PublicationStatus::Active {
                generation: entry.generation,
                expires: entry.expires,
            },
        }
    }
    fn renew(&self, now: Instant) -> capnp::Result<()> {
        self.update(now, None)
    }
    fn update(&self, now: Instant, visibility: Option<bool>) -> capnp::Result<()> {
        if !matches!(self.status(now), PublicationStatus::Active { .. }) {
            return Err(failed("Native publication no longer owned or live"));
        }
        // No capability hooks, destructors or notifications run under this
        // borrow. Renew shares the existing binding instead of cloning hooks.
        let (old, changed) = {
            let mut state = self.directory.0.borrow_mut();
            let generation = DiscoveryGeneration::after(state.next)?;
            let binding = state.entries[&self.key].binding.clone();
            let visible = visibility.unwrap_or(state.entries[&self.key].visible);
            state.next = generation.get();
            let old = state.entries.insert(
                self.key.clone(),
                Rc::new(Entry {
                    binding,
                    generation,
                    expires: now + self.lifetime,
                    visible,
                }),
            );
            self.generation.set(generation);
            (old, state.changed.clone())
        };
        drop(old);
        changed.notify_waiters();
        Ok(())
    }
    fn stop(&self) {
        if self.terminal.replace(Some(PublicationStatus::Stopped))
            != Some(PublicationStatus::Stopped)
        {
            self.directory
                .revoke(&self.key.0, self.key.1, self.generation.get());
        }
    }
}

/// Renews a publication at half its lifetime on a Tokio LocalSet. Dropping or
/// stopping the owner revokes only its current generation. Replacement,
/// revocation, closure and a missed expiry permanently end this lease.
#[must_use = "dropping the publication revokes it"]
pub struct Publication {
    lease: Rc<Lease>,
    task: tokio::task::JoinHandle<()>,
}
impl Publication {
    pub(super) fn suspend(&self) -> capnp::Result<()> {
        self.lease.update(Instant::now(), Some(false))
    }
    pub(super) fn replace(&self, binding: Binding) -> capnp::Result<()> {
        if binding.recipient != self.lease.key.1
            || !matches!(self.status(), PublicationStatus::Active { .. })
        {
            return Err(failed("Native publication no longer owned or live"));
        }
        let change = self.lease.directory.prepare(
            &self.lease.key.0,
            binding,
            Some(self.generation()),
            self.lease.lifetime,
        )?;
        self.lease.generation.set(change.generation);
        drop(change);
        Ok(())
    }
    pub fn generation(&self) -> DiscoveryGeneration {
        self.lease.generation.get()
    }
    pub fn status(&self) -> PublicationStatus {
        self.lease.status(Instant::now())
    }
    pub fn stop(&self) {
        self.task.abort();
        self.lease.stop();
    }
}
impl Drop for Publication {
    fn drop(&mut self) {
        self.stop();
    }
}
impl Directory {
    /// Publish and renew while the returned owner is held. Lifetime must be
    /// 1–60 seconds. Run inside a Tokio LocalSet. expected has publish()'s CAS
    /// semantics; a new owner can replace a live lease using its generation().
    pub fn maintain(
        &self,
        name: &str,
        binding: Binding,
        expected: Option<DiscoveryGeneration>,
        lifetime: Duration,
    ) -> capnp::Result<Publication> {
        if lifetime < Duration::from_secs(1) || lifetime > MAX_LIFETIME {
            return Err(failed("invalid Native renewal lifetime"));
        }
        let recipient = binding.recipient;
        let generation = self.publish(name, binding, expected, lifetime)?;
        let lease = Rc::new(Lease {
            directory: self.clone(),
            key: (name.into(), recipient),
            generation: Cell::new(generation),
            lifetime,
            terminal: Cell::new(None),
        });
        let running = lease.clone();
        let changed = self.0.borrow().changed.clone();
        let task = tokio::task::spawn_local(async move {
            let terminal = loop {
                let notified = changed.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                let status = running.status(Instant::now());
                let PublicationStatus::Active { expires, .. } = status else {
                    break status;
                };
                let renew_at = expires - running.lifetime / 2;
                tokio::select! {
                    biased;
                    _ = notified => {},
                    _ = tokio::time::sleep_until(renew_at) => {
                        if running.renew(Instant::now()).is_err() {
                            break match running.status(Instant::now()) {
                                PublicationStatus::Active { .. } => PublicationStatus::Exhausted,
                                status => status,
                            };
                        }
                    }
                }
            };
            running.terminal.set(Some(terminal));
            // Preserve a successor, but free this owner's slot on expiry or
            // generation exhaustion. The guard's last generation stays readable.
            running
                .directory
                .revoke(&running.key.0, running.key.1, running.generation.get());
        });
        Ok(Publication { lease, task })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn binding(host: u8) -> Binding {
        struct Provider;
        impl provisioner::Server for Provider {}
        Binding {
            host: [host; 32],
            recipient: [3; 32],
            address: "127.0.0.1:12345".parse().unwrap(),
            context: vec![],
            provider: capnp_rpc::new_client(Provider),
        }
    }
    async fn settle() {
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
    }
    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn renewal_owner_drop_and_successor_are_generation_safe() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let directory = Directory::default();
                let owner = directory
                    .maintain("service", binding(1), None, Duration::from_secs(2))
                    .unwrap();
                for generation in 2..=5 {
                    settle().await;
                    tokio::time::advance(Duration::from_secs(1)).await;
                    settle().await;
                    assert_eq!(owner.generation().get(), generation);
                    let found = resolve(&directory.client([3; 32]), [3; 32], "service")
                        .await
                        .unwrap();
                    assert_eq!(found.generation().get(), generation);
                }
                let replacement = directory
                    .maintain(
                        "service",
                        binding(2),
                        Some(owner.generation()),
                        Duration::from_secs(2),
                    )
                    .unwrap();
                assert_eq!(owner.status(), PublicationStatus::Superseded);
                drop(owner);
                settle().await;
                assert_eq!(
                    resolve(&directory.client([3; 32]), [3; 32], "service")
                        .await
                        .unwrap()
                        .binding()
                        .host,
                    [2; 32]
                );
                drop(replacement);
                assert!(resolve(&directory.client([3; 32]), [3; 32], "service")
                    .await
                    .is_err());
                tokio::time::advance(Duration::from_secs(10)).await;
                settle().await;
                assert!(directory.0.borrow().entries.is_empty());
            })
            .await;
    }
    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn revocation_expiry_close_and_exhaustion_end_renewal() {
        tokio::task::LocalSet::new()
            .run_until(async {
                for cause in 0..4 {
                    let directory = Directory::default();
                    let owner = directory
                        .maintain("service", binding(1), None, Duration::from_secs(2))
                        .unwrap();
                    settle().await;
                    let expected = match cause {
                        0 => {
                            assert!(directory.revoke("service", [3; 32], owner.generation()));
                            PublicationStatus::Revoked
                        }
                        1 => {
                            tokio::time::advance(Duration::from_secs(3)).await;
                            PublicationStatus::Expired
                        }
                        2 => {
                            directory.close();
                            PublicationStatus::Closed
                        }
                        _ => {
                            directory.0.borrow_mut().next = u64::MAX;
                            tokio::time::advance(Duration::from_secs(1)).await;
                            PublicationStatus::Exhausted
                        }
                    };
                    settle().await;
                    assert_eq!(owner.status(), expected);
                    assert!(directory.0.borrow().entries.is_empty());
                    tokio::time::advance(Duration::from_secs(10)).await;
                    settle().await;
                    assert_eq!(owner.generation().get(), 1);
                }
            })
            .await;
    }
    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn replay_tlc_owned_publication() {
        use reproto_test_support::verification::exploration;
        let config = include_str!("../../verification/NativePublication.cfg");
        let live = config.replace("SPECIFICATION Spec", "SPECIFICATION LiveSpec")
            + "\nPROPERTY OwnerStops\n";
        exploration::controls(
            "verification/NativePublication.tla",
            "native-publication",
            config,
            &[
                ("expiredRenew", "RenewalAuthority"),
                ("replaceRenew", "RenewalAuthority"),
                ("staleDrop", "SuccessorSurvives"),
            ],
            Some(&live),
        )
        .unwrap();
        let traces = exploration::traces(
            "verification/NativePublication.tla",
            "native-publication",
            config,
        )
        .unwrap();
        for trace in traces {
            let directory = Directory::default();
            let generation = directory
                .publish("service", binding(1), None, Duration::from_secs(1))
                .unwrap();
            let lease = Lease {
                directory: directory.clone(),
                key: ("service".into(), [3; 32]),
                generation: Cell::new(generation),
                lifetime: Duration::from_secs(1),
                terminal: Cell::new(None),
            };
            for state in trace {
                match state["event"] {
                    1 => assert_eq!(lease.renew(Instant::now()).is_ok(), state["accepted"] == 1),
                    2 => {
                        let current = directory.0.borrow().next;
                        let _ = directory
                            .publish(
                                "service",
                                binding(2),
                                DiscoveryGeneration::new(current),
                                Duration::from_secs(1),
                            )
                            .unwrap();
                    }
                    3 => tokio::time::advance(Duration::from_secs(1)).await,
                    4 => {
                        let current = directory.0.borrow().next;
                        assert!(directory.revoke(
                            "service",
                            [3; 32],
                            DiscoveryGeneration::new(current).unwrap()
                        ));
                    }
                    5 => directory.close(),
                    6 => lease.stop(),
                    _ => panic!("unknown publication action"),
                }
                let registry = directory.0.borrow();
                assert_eq!(registry.next, state["generation"]);
                assert_eq!(lease.generation.get().get(), state["owned"]);
                assert_eq!(registry.entries.len(), state["active"] as usize);
                if let Some(entry) = registry.entries.values().next() {
                    assert_eq!(entry.expires <= Instant::now(), state["expired"] == 1);
                }
            }
        }
    }
}

#[cfg(test)]
mod boundary;
