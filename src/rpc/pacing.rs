//! Reusable packet pacing, without rounding Linux deadlines to milliseconds.
#![forbid(unsafe_code)]

#[cfg(target_os = "linux")]
use std::task::Poll;
use std::{future::Future, pin::Pin};

#[derive(Default)]
pub(crate) struct Pacer {
    // Allocate a descriptor only when a packet actually needs to wait. Failure
    // permanently selects the portable timer, including descriptor exhaustion.
    #[cfg(target_os = "linux")]
    timer: Option<Result<PreciseTimer, ()>>,
    fallback: Option<Pin<Box<tokio::time::Sleep>>>,
}

impl Pacer {
    #[cfg(test)]
    pub(crate) fn portable() -> Self {
        Self {
            #[cfg(target_os = "linux")]
            timer: Some(Err(())),
            fallback: None,
        }
    }

    pub(crate) async fn wait_until(&mut self, at: std::time::Instant) {
        let deadline = tokio::time::Instant::from_std(at);
        let now = tokio::time::Instant::now();
        if deadline <= now {
            return;
        }
        // Keep the large runtime timer outside the packet driver's future and
        // reuse its registration. Already-due packets allocate nothing.
        let fallback = self
            .fallback
            .get_or_insert_with(|| Box::pin(tokio::time::sleep_until(deadline)));
        fallback.as_mut().reset(deadline);
        #[cfg(target_os = "linux")]
        let timer = self
            .timer
            .get_or_insert_with(|| PreciseTimer::new().map_err(|_| ()));
        #[cfg(target_os = "linux")]
        if let Ok(precise) = timer {
            if precise.arm(deadline - now).is_err() {
                *timer = Err(());
            }
        }
        #[cfg(target_os = "linux")]
        let mut precise_done = false;
        std::future::poll_fn(|cx| {
            #[cfg(target_os = "linux")]
            if !precise_done {
                if let Ok(precise) = timer {
                    if let Poll::Ready(result) = precise.poll_wait(cx) {
                        precise_done = true;
                        if result.is_err() {
                            *timer = Err(());
                        } else if tokio::time::Instant::now() >= deadline {
                            return Poll::Ready(());
                        }
                    }
                }
            }
            // Kernel readiness cannot advance a paused runtime clock. The
            // portable timer also retains virtual-clock simulation behavior.
            fallback.as_mut().poll(cx)
        })
        .await;
    }
}

#[cfg(target_os = "linux")]
struct PreciseTimer(tokio::io::unix::AsyncFd<std::os::fd::OwnedFd>);

#[cfg(target_os = "linux")]
impl PreciseTimer {
    fn new() -> std::io::Result<Self> {
        use rustix::time::{timerfd_create, TimerfdClockId, TimerfdFlags};
        let fd = timerfd_create(
            TimerfdClockId::Monotonic,
            TimerfdFlags::NONBLOCK | TimerfdFlags::CLOEXEC,
        )?;
        Ok(Self(tokio::io::unix::AsyncFd::with_interest(
            fd,
            tokio::io::Interest::READABLE,
        )?))
    }

    fn arm(&self, delay: std::time::Duration) -> std::io::Result<()> {
        use rustix::time::{timerfd_settime, Itimerspec, TimerfdTimerFlags, Timespec};
        timerfd_settime(
            self.0.get_ref(),
            TimerfdTimerFlags::empty(),
            &Itimerspec {
                it_interval: Timespec::default(),
                it_value: delay.try_into().map_err(std::io::Error::other)?,
            },
        )?;
        Ok(())
    }

    fn poll_wait(&self, cx: &mut std::task::Context<'_>) -> Poll<std::io::Result<()>> {
        // Rearming replaces the old deadline and clears its expiration count.
        // try_io clears any stale cached readiness on WouldBlock, so a
        // canceled wait cannot release a later packet before its new deadline.
        loop {
            let mut ready = std::task::ready!(self.0.poll_read_ready(cx))?;
            match ready.try_io(|fd| {
                rustix::io::read(fd.get_ref(), &mut [0u8; 8])
                    .map(|_| ())
                    .map_err(Into::into)
            }) {
                Ok(result) => return Poll::Ready(result),
                Err(_) => continue,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{task::Poll, time::Duration};
    use tokio::time::Instant;

    #[tokio::test(start_paused = true)]
    async fn due_packets_never_yield_and_future_packets_wait() {
        let now = Instant::now();
        let mut pacer = Pacer::default();
        assert_eq!(
            futures::poll!(std::pin::pin!(pacer.wait_until(now.into_std()))),
            Poll::Ready(())
        );
        assert_eq!(
            futures::poll!(std::pin::pin!(
                pacer.wait_until((now - Duration::from_micros(1)).into_std())
            )),
            Poll::Ready(())
        );
        #[cfg(target_os = "linux")]
        assert!(pacer.timer.is_none(), "due packets must not open a timerfd");
        assert!(
            pacer.fallback.is_none(),
            "due packets must not allocate a timer"
        );
        let future = pacer.wait_until((now + Duration::from_millis(10)).into_std());
        tokio::pin!(future);
        assert!(futures::poll!(&mut future).is_pending());
        tokio::time::advance(Duration::from_millis(9)).await;
        assert!(futures::poll!(&mut future).is_pending());
        tokio::time::advance(Duration::from_millis(2)).await;
        assert_eq!(futures::poll!(&mut future), Poll::Ready(()));
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn canceled_and_expired_waits_rearm_one_descriptor_without_early_release() {
        use std::os::fd::AsRawFd;
        let mut pacer = Pacer::default();
        {
            let canceled = pacer.wait_until((Instant::now() + Duration::from_millis(5)).into_std());
            tokio::pin!(canceled);
            assert!(futures::poll!(&mut canceled).is_pending());
        }
        let fd = pacer
            .timer
            .as_ref()
            .unwrap()
            .as_ref()
            .ok()
            .unwrap()
            .0
            .get_ref()
            .as_raw_fd();
        // Let the canceled timer expire, leaving a stale readiness notification.
        tokio::time::sleep(Duration::from_millis(10)).await;
        for delay in [Duration::from_millis(5), Duration::from_micros(200)] {
            let deadline = Instant::now() + delay;
            pacer.wait_until(deadline.into_std()).await;
            assert!(Instant::now() >= deadline);
            assert_eq!(
                pacer
                    .timer
                    .as_ref()
                    .unwrap()
                    .as_ref()
                    .ok()
                    .unwrap()
                    .0
                    .get_ref()
                    .as_raw_fd(),
                fd
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[tokio::test(start_paused = true)]
    async fn kernel_wakeup_cannot_advance_a_paused_runtime_clock() {
        let mut pacer = Pacer::default();
        let deadline = Instant::now() + Duration::from_millis(2);
        let future = pacer.wait_until(deadline.into_std());
        tokio::pin!(future);
        assert!(futures::poll!(&mut future).is_pending());
        // Make the kernel timer readable without advancing Tokio's clock.
        std::thread::sleep(Duration::from_millis(10));
        for _ in 0..4 {
            tokio::task::yield_now().await;
            assert!(futures::poll!(&mut future).is_pending());
        }
        tokio::time::advance(Duration::from_millis(3)).await;
        future.await;
    }

    #[cfg(target_os = "linux")]
    #[tokio::test(start_paused = true)]
    async fn rejected_timer_rearm_disables_kernel_path_and_preserves_deadline() {
        // A valid reactor descriptor that the kernel rejects as a timerfd.
        let (socket, _peer) = std::os::unix::net::UnixStream::pair().unwrap();
        socket.set_nonblocking(true).unwrap();
        let fd = tokio::io::unix::AsyncFd::with_interest(
            std::os::fd::OwnedFd::from(socket),
            tokio::io::Interest::READABLE,
        )
        .unwrap();
        let mut pacer = Pacer {
            timer: Some(Ok(PreciseTimer(fd))),
            ..Pacer::default()
        };
        for _ in 0..2 {
            let deadline = Instant::now() + Duration::from_millis(10);
            {
                let future = pacer.wait_until(deadline.into_std());
                tokio::pin!(future);
                assert!(futures::poll!(&mut future).is_pending());
                tokio::time::advance(Duration::from_millis(9)).await;
                assert!(futures::poll!(&mut future).is_pending());
                tokio::time::advance(Duration::from_millis(2)).await;
                future.await;
            }
            assert!(matches!(pacer.timer, Some(Err(()))));
        }
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn unrepresentable_kernel_delay_is_an_error() {
        let timer = PreciseTimer::new().unwrap();
        assert!(timer.arm(Duration::MAX).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn stopped_reactor_rejects_timer_registration() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        drop(runtime);
        let _entered = handle.enter();
        assert!(PreciseTimer::new().is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn stopped_timer_reactor_uses_the_live_runtime_deadline() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let timer = runtime.block_on(async { PreciseTimer::new().unwrap() });
        drop(runtime);
        let replacement = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        replacement.block_on(async {
            let mut pacer = Pacer {
                timer: Some(Ok(timer)),
                ..Pacer::default()
            };
            let deadline = Instant::now() + Duration::from_millis(2);
            pacer.wait_until(deadline.into_std()).await;
            assert!(Instant::now() >= deadline);
            assert!(matches!(pacer.timer, Some(Err(()))));
        });
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn descriptor_exhaustion_falls_back_to_runtime_timer() {
        const CHILD: &str = "CAPNTPROTO_PACING_FD_EXHAUSTION";
        if std::env::var_os(CHILD).is_none() {
            // Isolate the process-wide descriptor limit even under cargo test.
            assert!(std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "rpc::pacing::tests::descriptor_exhaustion_falls_back_to_runtime_timer",
                    "--nocapture"
                ])
                .env(CHILD, "1")
                .status()
                .unwrap()
                .success());
            return;
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        use rustix::process::{getrlimit, setrlimit, Resource, Rlimit};
        let saved = getrlimit(Resource::Nofile);
        setrlimit(
            Resource::Nofile,
            Rlimit {
                current: Some(0),
                maximum: saved.maximum,
            },
        )
        .unwrap();
        runtime.block_on(async {
            let mut pacer = Pacer::default();
            let deadline = Instant::now() + Duration::from_millis(2);
            pacer.wait_until(deadline.into_std()).await;
            assert!(Instant::now() >= deadline);
            assert!(matches!(pacer.timer, Some(Err(()))));
        });
        setrlimit(Resource::Nofile, saved).unwrap();
    }
}
