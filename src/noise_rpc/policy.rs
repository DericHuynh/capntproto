//! Bounded connection setup and opt-in recovery. These policies never replay RPC.
use super::{failed, Connector, VatId};
use crate::transport::AuthenticatedSession;
use capnp::{capability::Promise, Error, ErrorKind};
use std::{rc::Rc, time::Duration};

pub(super) fn within_deadline<T>(
    timeout: Duration,
    work: impl std::future::Future<Output = Result<T, super::RouteFailure>>,
) -> impl std::future::Future<Output = Result<T, super::RouteFailure>> {
    // Check the total budget before polling a dial again. timeout() polls its
    // inner future first and can start another attempt after a delayed wakeup.
    let deadline = tokio::time::Instant::now() + timeout;
    async move {
        tokio::select! {
            biased;
            _ = tokio::time::sleep_until(deadline) => Err(super::RouteFailure::new(
                super::FailureKind::Timeout, "Noise connection setup timed out")),
            result = work => result,
        }
    }
}

pub(crate) fn dial_error(error: std::io::Error) -> Error {
    use std::io::ErrorKind::*;
    match error.kind() {
        ConnectionRefused | ConnectionReset | ConnectionAborted | NotConnected | TimedOut
        | Interrupted | WouldBlock | NetworkDown | NetworkUnreachable | HostUnreachable => {
            Error::disconnected(error.to_string())
        }
        _ => failed(&error.to_string()),
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// Total connection setup budget, including retries and provisioning.
    pub connect_timeout: Duration,
    /// From receipt of a redirect or ThirdPartyAnswer until authenticated match.
    /// Method execution after the match is not subject to this deadline.
    pub answer_setup_timeout: Duration,
    /// New connect/attach operations may replace a failed generation. Existing
    /// capabilities remain broken and no outstanding request is resent.
    pub recover_failed_routes: bool,
    pub arbitration: Option<crate::noise_arbitration::Limits>,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(10),
            answer_setup_timeout: Duration::from_secs(10),
            recover_failed_routes: false,
            arbitration: None,
        }
    }
}
impl Options {
    pub(super) fn validate(self) -> capnp::Result<()> {
        for timeout in [self.connect_timeout, self.answer_setup_timeout] {
            if timeout.is_zero() || timeout > Duration::from_secs(60) {
                return Err(failed("Noise setup deadlines must be in (0, 60s]"));
            }
        }
        if let Some(limits) = self.arbitration {
            limits.validate()?;
        }
        Ok(())
    }
}

/// Retry only failures explicitly classified as disconnected or overloaded.
/// Invalid identities, permissions and malformed tickets must not be retried.
#[derive(Clone, Copy, Debug)]
pub struct RetryPolicy {
    pub attempts: u8,
    pub attempt_timeout: Duration,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
}
impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            attempts: 3,
            attempt_timeout: Duration::from_secs(2),
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(1),
        }
    }
}
impl RetryPolicy {
    fn validate(self) -> capnp::Result<()> {
        if self.attempts == 0
            || self.attempts > 8
            || self.attempt_timeout.is_zero()
            || self.attempt_timeout > Duration::from_secs(60)
            || self.initial_backoff.is_zero()
            || self.initial_backoff > self.max_backoff
            || self.max_backoff > Duration::from_secs(10)
        {
            return Err(failed("invalid Noise retry policy"));
        }
        Ok(())
    }
}

/// Decorates a connector, reacquiring a fresh reservation on each attempt.
/// Dropping the returned future cancels the current dial or backoff. There are
/// no detached tasks. The network still enforces its total setup deadline.
pub struct RetryingConnector {
    inner: Rc<dyn Connector>,
    policy: RetryPolicy,
}
impl RetryingConnector {
    pub fn new(inner: Rc<dyn Connector>, policy: RetryPolicy) -> capnp::Result<Self> {
        policy.validate()?;
        Ok(Self { inner, policy })
    }
}
impl Connector for RetryingConnector {
    fn connect(&self, peer: VatId) -> Promise<AuthenticatedSession, Error> {
        let inner = self.inner.clone();
        let policy = self.policy;
        Promise::from_future(async move {
            let mut backoff = policy.initial_backoff;
            for attempt in 1..=policy.attempts {
                let error =
                    match tokio::time::timeout(policy.attempt_timeout, inner.connect(peer)).await {
                        Ok(Ok(session)) => return Ok(session),
                        Ok(Err(error)) => error,
                        Err(_) => Error::disconnected("Noise dial attempt timed out".into()),
                    };
                if attempt == policy.attempts
                    || !matches!(error.kind, ErrorKind::Disconnected | ErrorKind::Overloaded)
                {
                    return Err(error);
                }
                tokio::time::sleep(backoff).await;
                backoff = backoff.saturating_mul(2).min(policy.max_backoff);
            }
            unreachable!("validated nonempty attempt budget")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::FutureExt;
    use std::cell::Cell;

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn setup_deadline_starts_before_the_driver_is_polled() {
        let started = Cell::new(false);
        let setup = within_deadline(Duration::from_secs(1), async {
            started.set(true);
            Ok(())
        });
        tokio::time::advance(Duration::from_secs(2)).await;
        assert_eq!(
            setup.await.unwrap_err().kind,
            super::super::FailureKind::Timeout
        );
        assert!(!started.get(), "expired setup must not start a dial");
    }

    struct Fails {
        attempts: Cell<u8>,
        kind: ErrorKind,
    }
    impl Connector for Fails {
        fn connect(&self, _: VatId) -> Promise<AuthenticatedSession, Error> {
            self.attempts.set(self.attempts.get() + 1);
            Promise::err(Error {
                kind: self.kind,
                ..Error::failed("dial failed".into())
            })
        }
    }
    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn retries_back_off_and_stop_at_budget_or_permanent_failure() {
        for kind in [
            ErrorKind::Disconnected,
            ErrorKind::Overloaded,
            ErrorKind::Failed,
            ErrorKind::Unimplemented,
        ] {
            let inner = Rc::new(Fails {
                attempts: Cell::new(0),
                kind,
            });
            let connector = RetryingConnector::new(inner.clone(), RetryPolicy::default()).unwrap();
            let start = tokio::time::Instant::now();
            assert!(connector.connect([1; 32]).await.is_err());
            let retryable = matches!(kind, ErrorKind::Disconnected | ErrorKind::Overloaded);
            assert_eq!(inner.attempts.get(), if retryable { 3 } else { 1 });
            assert_eq!(
                start.elapsed(),
                Duration::from_millis(if retryable { 300 } else { 0 })
            );
        }
    }
    struct Pending(Rc<Cell<usize>>, Rc<Cell<usize>>);
    struct Active(Rc<Cell<usize>>);
    impl Drop for Active {
        fn drop(&mut self) {
            self.0.set(self.0.get() - 1);
        }
    }
    impl Connector for Pending {
        fn connect(&self, _: VatId) -> Promise<AuthenticatedSession, Error> {
            self.0.set(self.0.get() + 1);
            self.1.set(self.1.get() + 1);
            let active = Active(self.1.clone());
            Promise::from_future(async move {
                let _active = active;
                futures::future::pending().await
            })
        }
    }
    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn timeout_drops_each_attempt_and_cancellation_drops_current_attempt() {
        let count = Rc::new(Cell::new(0));
        let active = Rc::new(Cell::new(0));
        let connector = RetryingConnector::new(
            Rc::new(Pending(count.clone(), active.clone())),
            RetryPolicy::default(),
        )
        .unwrap();
        let start = tokio::time::Instant::now();
        assert!(connector.connect([1; 32]).await.is_err());
        assert_eq!(count.get(), 3);
        assert_eq!(active.get(), 0);
        assert_eq!(start.elapsed(), Duration::from_millis(6300));
        let mut pending = connector.connect([1; 32]);
        assert!((&mut pending).now_or_never().is_none());
        assert_eq!(active.get(), 1);
        drop(pending);
        assert_eq!(active.get(), 0);
        tokio::time::advance(Duration::from_secs(60)).await;
        assert_eq!(count.get(), 4);
    }
    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn cancellation_during_backoff_does_not_redial() {
        let inner = Rc::new(Fails {
            attempts: Cell::new(0),
            kind: ErrorKind::Disconnected,
        });
        let connector = RetryingConnector::new(inner.clone(), RetryPolicy::default()).unwrap();
        let mut pending = connector.connect([1; 32]);
        assert!((&mut pending).now_or_never().is_none());
        drop(pending);
        tokio::time::advance(Duration::from_secs(60)).await;
        assert_eq!(inner.attempts.get(), 1);
    }
}
