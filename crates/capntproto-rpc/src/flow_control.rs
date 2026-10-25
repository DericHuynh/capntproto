use alloc::{boxed::Box, string::ToString, vec::Vec};
use capnp::capability::Promise;
use capnp::Error;

use alloc::rc::Rc;
use capntproto_rpc_engine::completion::shared as oneshot;
use core::cell::RefCell;
use core::time::Duration;
use futures::TryFutureExt;

use crate::task_set::{TaskReaper, TaskSet, TaskSetHandle};

use capntproto_rpc_engine::flow_control::Window;

pub(crate) const DEFAULT_WINDOW_SIZE: usize = 65536;

#[cfg(feature = "std")]
#[derive(Clone)]
pub(crate) enum Policy {
    Fixed(usize),
    Variable(Rc<dyn Fn() -> usize>),
    #[cfg(feature = "std")]
    Adaptive(usize),
}

#[cfg(feature = "std")]
impl Policy {
    pub(crate) fn controller(&self) -> (Box<dyn crate::FlowController>, Promise<(), Error>) {
        match self {
            Self::Fixed(size) => crate::new_fixed_window_flow_controller(*size),
            Self::Variable(getter) => {
                let getter = getter.clone();
                crate::new_variable_window_flow_controller(move || getter())
            }
            #[cfg(feature = "std")]
            Self::Adaptive(size) => crate::new_adaptive_flow_controller(*size),
        }
    }
}

enum State {
    Running(Vec<oneshot::Sender<Result<(), Error>>>),
    Failed(Error),
}

struct WindowState {
    credit: Window,
    state: State,
}

impl WindowState {
    fn needs_window(&self) -> bool {
        matches!(self.state, State::Running(_)) && self.credit.needs_window()
    }

    fn is_ready(&self) -> bool {
        self.credit.is_ready()
    }

    fn take_ready(&mut self) -> Vec<oneshot::Sender<Result<(), Error>>> {
        if self.is_ready() {
            if let State::Running(waiters) = &mut self.state {
                return core::mem::take(waiters);
            }
        }
        Vec::new()
    }
}

pub(crate) struct WindowFlowController {
    inner: Rc<RefCell<WindowState>>,
    tasks: TaskSetHandle<Error>,
    getter: Option<Rc<dyn Fn() -> usize>>,
    clock: Option<Rc<dyn Fn() -> Duration>>,
}

struct Reaper {
    inner: Rc<RefCell<WindowState>>,
}

impl TaskReaper<Error> for Reaper {
    fn task_failed(&mut self, error: Error) {
        let waiters = {
            let mut inner = self.inner.borrow_mut();
            if !matches!(inner.state, State::Running(_)) {
                return;
            }
            let State::Running(waiters) =
                core::mem::replace(&mut inner.state, State::Failed(error.clone()))
            else {
                unreachable!()
            };
            waiters
        };
        for waiter in waiters {
            let _ = waiter.send(Err(error.clone()));
        }
    }
}

impl WindowFlowController {
    pub(crate) fn new(
        window_size: usize,
        getter: Option<Rc<dyn Fn() -> usize>>,
        clock: Option<Rc<dyn Fn() -> Duration>>,
    ) -> (Self, Promise<(), Error>) {
        let inner = Rc::new(RefCell::new(WindowState {
            credit: if clock.is_some() {
                Window::adaptive(window_size)
            } else {
                Window::fixed(window_size)
            },
            state: State::Running(vec![]),
        }));
        let (tasks, task_future) = TaskSet::new(Box::new(Reaper {
            inner: inner.clone(),
        }));
        (
            Self {
                inner,
                tasks,
                getter,
                clock,
            },
            Promise::from_future(task_future),
        )
    }
}

impl crate::FlowController for WindowFlowController {
    fn send(
        &mut self,
        message: Box<dyn crate::OutgoingMessage>,
        ack: Promise<(), Error>,
    ) -> Promise<(), Error> {
        let size = (message.size_in_words() as u128) * 8;
        // Sending must be immediate, even if earlier calls failed or blocked.
        // Do not hold our state borrow while invoking an external message/getter.
        let _ = message.send_detached();
        let now = self.clock.as_ref().map_or(Duration::ZERO, |clock| clock());
        let (receipt, needs_window) = {
            let mut inner = self.inner.borrow_mut();
            let receipt = inner.credit.sent(size, now);
            (receipt, inner.needs_window())
        };
        let receipt = match receipt {
            Ok(receipt) => receipt,
            Err(error) => {
                let error = Error::failed(error.to_string());
                Reaper {
                    inner: self.inner.clone(),
                }
                .task_failed(error.clone());
                // The message was already handed to the transport. A rejected
                // clock/accounting input must not discard its acknowledgement
                // or let wait_all_acked report completion before it settles.
                self.tasks.add(ack);
                return Promise::err(error);
            }
        };
        // C++ short-circuits the query while one largest message fits, and
        // never queries a failed stream. Besides avoiding syscalls, this avoids
        // prematurely caching an unavailable socket hint.
        let window = self
            .getter
            .as_ref()
            .filter(|_| needs_window)
            .map(|get| get());
        let ready = {
            let mut inner = self.inner.borrow_mut();
            if let Some(window) = window {
                inner.credit.set_size(window);
            }
            inner.is_ready()
        };

        let inner = self.inner.clone();
        let getter = self.getter.clone();
        let clock = self.clock.clone();
        self.tasks.add(async move {
            ack.await?;
            let now = clock.as_ref().map_or(Duration::ZERO, |clock| clock());
            let needs_window = {
                let mut inner = inner.borrow_mut();
                inner
                    .credit
                    .ack(receipt, now)
                    .map_err(|error| Error::failed(error.to_string()))?;
                inner.needs_window()
            };
            let window = getter.as_ref().filter(|_| needs_window).map(|get| get());
            release_ready(&inner, window);
            Ok(())
        });

        let mut inner = self.inner.borrow_mut();
        match &mut inner.state {
            State::Running(waiters) => {
                if ready {
                    Promise::ok(())
                } else {
                    let (snd, rcv) = oneshot::channel();
                    waiters.push(snd);
                    Promise::from_future(async {
                        rcv.await
                            .map_err(|_| Error::failed("oneshot was canceled".into()))?
                    })
                }
            }
            State::Failed(error) => Promise::err(error.clone()),
        }
    }

    fn wait_all_acked(&mut self) -> Promise<(), Error> {
        // Include successful and failed acknowledgements. Credit alone cannot
        // signal drain: failed calls intentionally leave their credit charged.
        Promise::from_future(self.tasks.on_empty().map_err(crate::canceled_to_error))
    }
}

impl Drop for WindowFlowController {
    fn drop(&mut self) {
        // Credit promises are not delivery receipts. The driver still owns all
        // outstanding acknowledgements and completes once they settle.
        let waiters = match &mut self.inner.borrow_mut().state {
            State::Running(waiters) => core::mem::take(waiters),
            State::Failed(_) => Vec::new(),
        };
        for waiter in waiters {
            let _ = waiter.send(Ok(()));
        }
    }
}

// A completion can synchronously wake caller code. Detach every waiter before
// sending so a wake may inspect or use the controller without a RefCell panic.
fn release_ready(inner: &RefCell<WindowState>, window: Option<usize>) {
    let waiters = {
        let mut inner = inner.borrow_mut();
        if let Some(window) = window {
            inner.credit.set_size(window);
        }
        inner.take_ready()
    };
    for waiter in waiters {
        let _ = waiter.send(Ok(()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        future::Future,
        pin::Pin,
        sync::Arc,
        task::{Context, Poll, Wake, Waker},
    };
    thread_local! { static ON_WAKE: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None); }
    struct Reenter;
    impl Wake for Reenter {
        fn wake(self: Arc<Self>) {
            let callback = ON_WAKE.with(|slot| slot.borrow_mut().take());
            if let Some(callback) = callback {
                callback();
            }
        }
    }
    #[test]
    fn credit_ready_failure_and_drop_wake_outside_controller_borrows() {
        for mode in 0..3 {
            let (controller, _driver) = WindowFlowController::new(64, None, None);
            let inner = controller.inner.clone();
            let mut controller = Some(controller);
            let (sender, mut receiver) = oneshot::channel();
            {
                let mut state = inner.borrow_mut();
                let State::Running(waiters) = &mut state.state else {
                    unreachable!()
                };
                waiters.push(sender);
            }
            // End the temporary borrow before installing the reentrant wake.
            let seen = Rc::new(std::cell::Cell::new(false));
            let inspect = inner.clone();
            let flag = seen.clone();
            ON_WAKE.with(|slot| {
                *slot.borrow_mut() = Some(Box::new(move || {
                    let state = inspect
                        .try_borrow()
                        .expect("flow callbacks run outside state borrows");
                    assert_eq!(matches!(state.state, State::Failed(_)), mode == 1);
                    flag.set(true);
                }))
            });
            let waker = Waker::from(Arc::new(Reenter));
            let mut cx = Context::from_waker(&waker);
            assert!(Pin::new(&mut receiver).poll(&mut cx).is_pending());
            match mode {
                0 => release_ready(&inner, None),
                1 => Reaper {
                    inner: inner.clone(),
                }
                .task_failed(Error::failed("test failure".into())),
                _ => drop(controller.take()),
            }
            assert!(seen.get());
            let Poll::Ready(Ok(result)) = Pin::new(&mut receiver).poll(&mut cx) else {
                panic!("credit completion")
            };
            assert_eq!(result.is_err(), mode == 1);
        }
    }
}
