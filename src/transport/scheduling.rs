//! Local, bounded scheduling policy. These controls grant no remote authority
//! and do not change quiche's congestion control, packet pacing or reliability.
use std::{
    cell::{Cell, RefCell},
    io,
    rc::{Rc, Weak},
    sync::Arc,
    time::Duration,
};
use tokio::{sync::Notify, time::Instant};

/// Refill one datagram credit per interval, up to `burst` credits. Credit limits
/// admission to quiche, not UDP bytes or delivery. Existing queued datagrams wait.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DatagramPacing {
    pub interval: Duration,
    pub burst: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Schedule {
    /// Maximum packets sent before polling other work: 1–64, default 16.
    pub packet_burst: u8,
    /// None preserves unrestricted admission from the bounded datagram queue.
    pub datagrams: Option<DatagramPacing>,
}
impl Default for Schedule {
    fn default() -> Self {
        Self {
            packet_burst: 16,
            datagrams: None,
        }
    }
}
impl Schedule {
    fn validate(self) -> io::Result<()> {
        if self.packet_burst == 0
            || self.packet_burst > 64
            || self.datagrams.is_some_and(|p| {
                p.burst == 0
                    || p.burst > 64
                    || p.interval < Duration::from_micros(100)
                    || p.interval > Duration::from_secs(1)
            })
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid Native scheduling policy",
            ));
        }
        Ok(())
    }
}
/// Local counters, not acknowledgement or application-execution receipts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScheduleStats {
    pub packet_attempts: u64,
    pub datagram_attempts: u64,
    pub burst_yields: u64,
}
struct Bucket {
    policy: Option<DatagramPacing>,
    credits: u8,
    updated: Instant,
}
impl Bucket {
    fn refill(&mut self, now: Instant) {
        let Some(policy) = self.policy else { return };
        let elapsed = now.saturating_duration_since(self.updated);
        let ticks = elapsed.as_nanos() / policy.interval.as_nanos();
        if ticks >= u128::from(policy.burst - self.credits) {
            // A full bucket carries no extra fractional or idle-time credit.
            self.credits = policy.burst;
            self.updated = now;
        } else if ticks > 0 {
            self.credits += ticks as u8;
            self.updated += policy.interval * ticks as u32;
        }
    }
    fn configure(&mut self, policy: Option<DatagramPacing>, now: Instant) {
        if self.policy == policy {
            return;
        }
        self.refill(now);
        self.credits = policy.map_or(0, |p| {
            if self.policy.is_none() {
                p.burst
            } else {
                self.credits.min(p.burst)
            }
        });
        self.policy = policy;
        self.updated = now;
    }
    fn take(&mut self, now: Instant) -> bool {
        self.refill(now);
        if self.policy.is_none() {
            return true;
        }
        if self.credits == 0 {
            return false;
        }
        self.credits -= 1;
        true
    }
    fn deadline(&mut self, now: Instant) -> Option<Instant> {
        self.refill(now);
        self.policy
            .filter(|_| self.credits == 0)
            .map(|p| self.updated + p.interval)
    }
}
struct State {
    policy: Cell<Schedule>,
    bucket: RefCell<Bucket>,
    closed: Cell<bool>,
    stats: Cell<ScheduleStats>,
    changed: Arc<Notify>,
}
impl State {
    fn admit(&self, now: Instant) -> bool {
        if self.closed.get() || !self.bucket.borrow_mut().take(now) {
            return false;
        }
        let mut stats = self.stats.get();
        stats.datagram_attempts = stats.datagram_attempts.saturating_add(1);
        self.stats.set(stats);
        true
    }
}
/// A weak control for one session. Clones neither keep the session alive nor
/// carry policy into a reconnected/arbitrated replacement session.
#[derive(Clone)]
pub struct Scheduling(Weak<State>);
impl Scheduling {
    fn state(&self) -> io::Result<Rc<State>> {
        self.0.upgrade().filter(|s| !s.closed.get()).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::BrokenPipe,
                "Native scheduling session closed",
            )
        })
    }
    /// Synchronous policy update; wake the driver even when datagrams are waiting
    /// for credit. Reapplying a policy does not refill its bucket. Changing a
    /// limited policy carries at most its remaining credit into the new one.
    pub fn configure(&self, policy: Schedule) -> io::Result<()> {
        policy.validate()?;
        let state = self.state()?;
        state
            .bucket
            .borrow_mut()
            .configure(policy.datagrams, Instant::now());
        state.policy.set(policy);
        state.changed.notify_waiters();
        Ok(())
    }
    pub fn policy(&self) -> io::Result<Schedule> {
        Ok(self.state()?.policy.get())
    }
    pub fn stats(&self) -> io::Result<ScheduleStats> {
        Ok(self.state()?.stats.get())
    }
}
pub(super) struct Driver(Rc<State>);
pub(super) fn pair() -> (Scheduling, Driver) {
    let state = Rc::new(State {
        policy: Cell::new(Schedule::default()),
        bucket: RefCell::new(Bucket {
            policy: None,
            credits: 0,
            updated: Instant::now(),
        }),
        closed: Cell::new(false),
        stats: Cell::new(ScheduleStats::default()),
        changed: Arc::new(Notify::new()),
    });
    (Scheduling(Rc::downgrade(&state)), Driver(state))
}
impl Drop for Driver {
    fn drop(&mut self) {
        self.0.closed.set(true);
        self.0.changed.notify_waiters();
    }
}
impl Driver {
    pub(super) fn changed(&self) -> Arc<Notify> {
        self.0.changed.clone()
    }
    pub(super) fn admit(&self, now: Instant) -> bool {
        self.0.admit(now)
    }
    pub(super) fn deadline(&self, now: Instant) -> Option<Instant> {
        self.0.bucket.borrow_mut().deadline(now)
    }
    pub(super) fn burst(&self) -> Burst {
        Burst(self.0.policy.get().packet_burst)
    }
    pub(super) fn sent(&self) {
        let mut stats = self.0.stats.get();
        stats.packet_attempts = stats.packet_attempts.saturating_add(1);
        self.0.stats.set(stats);
    }
    pub(super) fn yielded(&self) {
        let mut stats = self.0.stats.get();
        stats.burst_yields = stats.burst_yields.saturating_add(1);
        self.0.stats.set(stats);
    }
}
pub(super) struct Burst(u8);
impl Burst {
    pub(super) fn permit(&mut self) -> bool {
        if self.0 == 0 {
            return false;
        }
        self.0 -= 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn policy(burst: u8) -> Schedule {
        Schedule {
            packet_burst: 2,
            datagrams: Some(DatagramPacing {
                interval: Duration::from_millis(10),
                burst,
            }),
        }
    }
    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn replay_tlc_scheduling() {
        use capntproto_test_support::verification::exploration;
        let config = include_str!("../../verification/NativeScheduling.cfg");
        exploration::controls(
            "verification/NativeScheduling.tla",
            "native-scheduling",
            config,
            &[
                ("freeCredit", "CreditBound"),
                ("burstOverflow", "BurstBound"),
                ("afterClose", "NoAfterClose"),
            ],
            None,
        )
        .unwrap();
        let traces = exploration::traces(
            "verification/NativeScheduling.tla",
            "native-scheduling",
            config,
        )
        .unwrap();
        for trace in traces {
            let (control, driver) = pair();
            let state = driver.0.clone();
            let mut driver = Some(driver);
            control.configure(policy(2)).unwrap();
            let mut burst = driver.as_ref().unwrap().burst();
            let mut packets = 0;
            let mut granted = 0;
            for model in trace {
                match model["event"] {
                    1 => granted = u64::from(state.admit(Instant::now())),
                    2 => {
                        tokio::time::advance(Duration::from_millis(10)).await;
                        driver.as_ref().unwrap().deadline(Instant::now());
                    }
                    3 => control.configure(policy(1)).unwrap(),
                    4 => control.configure(control.policy().unwrap()).unwrap(),
                    5 => {
                        if burst.permit() {
                            packets += 1;
                            driver.as_ref().unwrap().sent();
                        }
                    }
                    6 => {
                        burst = driver.as_ref().unwrap().burst();
                        packets = 0;
                        driver.as_ref().unwrap().yielded();
                    }
                    7 => {
                        driver.take();
                    }
                    e => panic!("unexpected event {e}"),
                }
                assert_eq!(
                    state.bucket.borrow().credits as u64,
                    model["credits"],
                    "{model:?}"
                );
                assert_eq!(
                    state.stats.get().datagram_attempts,
                    model["admitted"],
                    "{model:?}"
                );
                assert_eq!(granted, model["grant"], "{model:?}");
                assert_eq!(packets, model["packets"], "{model:?}");
                assert_eq!(control.policy().is_err(), model["closed"] == 1);
                if model["closed"] == 1 {
                    assert_eq!(
                        control.configure(policy(2)).unwrap_err().kind(),
                        io::ErrorKind::BrokenPipe
                    );
                }
            }
        }
    }
    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn fractional_refill_idle_cap_and_reconfiguration_preserve_credit() {
        let (control, driver) = pair();
        control.configure(policy(2)).unwrap();
        assert!(driver.admit(Instant::now()));
        assert!(driver.admit(Instant::now()));
        assert!(!driver.admit(Instant::now()));
        let next = driver.deadline(Instant::now()).unwrap();
        tokio::time::advance(Duration::from_millis(9)).await;
        assert!(!driver.admit(Instant::now()));
        assert_eq!(driver.deadline(Instant::now()), Some(next));
        tokio::time::advance(Duration::from_millis(6)).await;
        assert!(driver.admit(Instant::now()));
        assert_eq!(
            driver.deadline(Instant::now()),
            Some(next + Duration::from_millis(10))
        );
        // An identical policy cannot add credit or reset the partial interval.
        control.configure(policy(2)).unwrap();
        assert_eq!(
            driver.deadline(Instant::now()),
            Some(next + Duration::from_millis(10))
        );
        // Enlarging the burst carries credit, rather than granting a new burst.
        control.configure(policy(4)).unwrap();
        assert!(!driver.admit(Instant::now()));
        tokio::time::advance(Duration::from_secs(3600)).await;
        for _ in 0..4 {
            assert!(driver.admit(Instant::now()));
        }
        assert!(!driver.admit(Instant::now()));
        assert_eq!(
            driver.deadline(Instant::now()),
            Some(Instant::now() + Duration::from_millis(10))
        );
        control.configure(Schedule::default()).unwrap();
        assert!(driver.admit(Instant::now()));
        assert_eq!(driver.deadline(Instant::now()), None);
        drop(driver);
        assert!(control.stats().is_err());
    }
    #[tokio::test(flavor = "current_thread")]
    async fn policy_validation_and_driver_cancellation_before_first_poll() {
        let (control, driver) = pair();
        for packet_burst in [0, 65] {
            assert!(control
                .configure(Schedule {
                    packet_burst,
                    ..Schedule::default()
                })
                .is_err());
        }
        for (burst, interval) in [(0, 100), (65, 100), (1, 99), (1, 1_000_001)] {
            assert!(control
                .configure(Schedule {
                    datagrams: Some(DatagramPacing {
                        interval: Duration::from_micros(interval),
                        burst
                    }),
                    ..Schedule::default()
                })
                .is_err());
        }
        assert_eq!(control.policy().unwrap(), Schedule::default());
        let future = async move {
            let _driver = driver;
            std::future::pending::<()>().await;
        };
        drop(future);
        assert!(control.configure(Schedule::default()).is_err());
        let (fresh, _driver) = pair();
        fresh.configure(policy(1)).unwrap();
        assert!(control.configure(policy(2)).is_err());
        assert_eq!(fresh.policy().unwrap(), policy(1));
    }
}
