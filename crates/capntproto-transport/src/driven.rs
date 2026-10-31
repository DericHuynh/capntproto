//! Owned drivers run in the caller's task. Successful scope completion requests
//! shutdown and drains local output. Dropping a scope cancels its owned IO.
use core::{
    future::{poll_fn, Future},
    pin::{pin, Pin},
    task::{Context, Poll},
};

#[derive(Debug, PartialEq, Eq)]
pub enum Error<E> {
    Transport(E),
    Stopped,
}

impl<E: core::fmt::Display> core::fmt::Display for Error<E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Transport(error) => write!(f, "transport: {error}"),
            Self::Stopped => f.write_str("transport stopped before the application completed"),
        }
    }
}
impl<E: core::error::Error + 'static> core::error::Error for Error<E> {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Transport(error) => Some(error),
            Self::Stopped => None,
        }
    }
}

pub trait Driver: Future<Output = Result<(), Self::Error>> {
    type Error;
    /// Request bounded protocol/local IO shutdown. The future then completes
    /// after accepted output has drained; cancellation remains immediate Drop.
    fn start_shutdown(self: Pin<&mut Self>);
}

pin_project_lite::pin_project! {
    pub struct Owned<F,S> {
        #[pin]
        future:F,
        shutdown:S,
    }
}
impl<F, S> Owned<F, S> {
    /// Wrap an owned protocol future with its idempotent shutdown request.
    pub fn new(future: F, shutdown: S) -> Self {
        Self { future, shutdown }
    }
}
impl<F: Future, S> Future for Owned<F, S> {
    type Output = F::Output;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.project().future.poll(cx)
    }
}
impl<F, S, E> Driver for Owned<F, S>
where
    F: Future<Output = Result<(), E>>,
    S: FnMut(),
{
    type Error = E;
    fn start_shutdown(self: Pin<&mut Self>) {
        (self.project().shutdown)();
    }
}

/// Poll application and transport in one scope. Once the application completes,
/// retain its output while the driver drains its close operation. Apply the
/// runtime's timeout to this whole future when a bounded shutdown is required.
/// Read/write completion futures stay retained inside the driver across polls.
pub async fn run_until<D: Driver, A: Future>(
    driver: D,
    application: A,
) -> Result<A::Output, Error<D::Error>> {
    let mut driver = pin!(driver);
    let mut application = pin!(Some(application));
    let mut output = None;
    poll_fn(|cx| {
        if output.is_none() {
            if let Poll::Ready(value) = application.as_mut().as_pin_mut().unwrap().poll(cx) {
                output = Some(value);
                // Completed async frames may still own stream/endpoint guards.
                // Drop them in place before requesting driver shutdown.
                application.as_mut().set(None);
                driver.as_mut().start_shutdown();
            }
        }
        match driver.as_mut().poll(cx) {
            Poll::Ready(Ok(())) => {
                if let Some(value) = output.take() {
                    return Poll::Ready(Ok(value));
                }
                match application.as_mut().as_pin_mut().unwrap().poll(cx) {
                    Poll::Ready(value) => Poll::Ready(Ok(value)),
                    Poll::Pending => Poll::Ready(Err(Error::Stopped)),
                }
            }
            Poll::Ready(Err(e)) => Poll::Ready(Err(Error::Transport(e))),
            Poll::Pending => Poll::Pending,
        }
    })
    .await
}
pub trait DriverExt: Driver + Sized {
    fn run_until<A: Future>(
        self,
        application: A,
    ) -> impl Future<Output = Result<A::Output, Error<Self::Error>>> {
        run_until(self, application)
    }
}
impl<D: Driver> DriverExt for D {}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use core::cell::Cell;
    use std::rc::Rc;

    fn poll<F: Future>(future: Pin<&mut F>) -> Poll<F::Output> {
        future.poll(&mut Context::from_waker(std::task::Waker::noop()))
    }

    #[test]
    fn borrowed_non_send_scope_drops_application_before_shutdown() {
        struct Application<'a>(&'a Cell<u8>);
        impl Future for Application<'_> {
            type Output = u8;
            fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<u8> {
                assert_eq!(self.0.replace(1), 0);
                Poll::Ready(42)
            }
        }
        impl Drop for Application<'_> {
            fn drop(&mut self) {
                assert_eq!(self.0.replace(2), 1);
            }
        }
        let state = Cell::new(0);
        let non_send = Rc::new(());
        let driver = Owned::new(
            async {
                assert_eq!(state.get(), 3);
                Ok::<_, ()>(())
            },
            || {
                let _ = &non_send;
                assert_eq!(state.replace(3), 2);
            },
        );
        let mut scope = pin!(driver.run_until(Application(&state)));
        assert_eq!(poll(scope.as_mut()), Poll::Ready(Ok(42)));
    }

    #[test]
    fn retains_output_while_shutdown_is_pending_and_never_repolls_application() {
        let shutdown = Cell::new(false);
        let ready = Cell::new(false);
        let app_polls = Cell::new(0);
        let driver = Owned::new(
            poll_fn(|_| {
                assert!(shutdown.get());
                if ready.get() {
                    Poll::Ready(Ok::<_, ()>(()))
                } else {
                    Poll::Pending
                }
            }),
            || shutdown.set(true),
        );
        let application = poll_fn(|_| {
            assert_eq!(app_polls.replace(1), 0);
            Poll::Ready(17)
        });
        let mut scope = pin!(driver.run_until(application));
        assert_eq!(poll(scope.as_mut()), Poll::Pending);
        ready.set(true);
        assert_eq!(poll(scope.as_mut()), Poll::Ready(Ok(17)));
    }

    #[test]
    fn driver_may_make_application_ready_on_its_last_poll() {
        let ready = Cell::new(false);
        let driver = Owned::new(
            async {
                ready.set(true);
                Ok::<_, ()>(())
            },
            || panic!("already stopped"),
        );
        let application = poll_fn(|_| {
            if ready.get() {
                Poll::Ready(11)
            } else {
                Poll::Pending
            }
        });
        assert_eq!(
            poll(pin!(driver.run_until(application))),
            Poll::Ready(Ok(11))
        );
    }

    #[test]
    fn early_stop_and_transport_errors_are_distinct() {
        let driver = Owned::new(async { Ok::<_, u8>(()) }, || {});
        assert_eq!(
            poll(pin!(driver.run_until(core::future::pending::<()>()))),
            Poll::Ready(Err(Error::Stopped))
        );
        let driver = Owned::new(async { Err::<(), _>(7) }, || {});
        assert_eq!(
            poll(pin!(driver.run_until(core::future::ready(3)))),
            Poll::Ready(Err(Error::Transport(7)))
        );
    }

    #[test]
    fn canceling_scope_drops_owned_operations_without_waiting_for_shutdown() {
        struct DropFlag<'a>(&'a Cell<bool>);
        impl Drop for DropFlag<'_> {
            fn drop(&mut self) {
                self.0.set(true);
            }
        }
        let dropped = Cell::new(false);
        let flag = DropFlag(&dropped);
        let driver = Owned::new(
            async move {
                let _flag = flag;
                core::future::pending::<Result<(), ()>>().await
            },
            || panic!("cancellation does not run graceful shutdown"),
        );
        let mut scope = std::boxed::Box::pin(driver.run_until(core::future::pending::<()>()));
        assert_eq!(poll(scope.as_mut()), Poll::Pending);
        drop(scope);
        assert!(dropped.get());
    }
}
