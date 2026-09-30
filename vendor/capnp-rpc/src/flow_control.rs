use capnp::capability::Promise;
use capnp::Error;

use futures::channel::oneshot;
use futures::TryFutureExt;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use crate::task_set::{TaskReaper, TaskSet, TaskSetHandle};

mod adaptive;
use adaptive::AdaptiveWindow;

pub(crate) const DEFAULT_WINDOW_SIZE: usize = 65536;

#[derive(Clone)]
pub(crate) enum Policy {
    Fixed(usize),
    Variable(Rc<dyn Fn() -> usize>),
    Adaptive(usize),
}

impl Policy {
    pub(crate) fn controller(&self) -> (Box<dyn crate::FlowController>, Promise<(), Error>) {
        match self {
            Self::Fixed(size) => crate::new_fixed_window_flow_controller(*size),
            Self::Variable(getter) => {
                let getter = getter.clone();
                crate::new_variable_window_flow_controller(move || getter())
            }
            Self::Adaptive(size) => crate::new_adaptive_flow_controller(*size),
        }
    }
}

enum State {
    Running(Vec<oneshot::Sender<Result<(), Error>>>),
    Failed(Error),
}

struct WindowState {
    window_size: usize,
    // Widen before converting words to bytes and accounting multiple sends.
    // Even synthetic messages reporting usize::MAX words cannot wrap the window.
    in_flight: u128,
    max_message_size: u128,
    adaptive: Option<AdaptiveWindow>,
    state: State,
}

impl WindowState {
    fn needs_window(&self) -> bool {
        matches!(self.state, State::Running(_)) && self.in_flight > self.max_message_size
    }

    fn is_ready(&self) -> bool {
        // One extra largest message avoids stop-and-wait for oversized messages.
        // C++ adaptive readiness is strict even for a zero initial window;
        // fixed/variable controllers additionally allow one message at zero.
        (self.adaptive.is_none() && self.in_flight <= self.max_message_size)
            || self.in_flight < self.max_message_size + self.window_size as u128
    }

    fn release_ready(&mut self) {
        if self.is_ready() {
            if let State::Running(waiters) = &mut self.state {
                for waiter in std::mem::take(waiters) {
                    let _ = waiter.send(Ok(()));
                }
            }
        }
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
        let mut inner = self.inner.borrow_mut();
        if let State::Running(ref mut waiters) = &mut inner.state {
            for waiter in std::mem::take(waiters) {
                let _ = waiter.send(Err(error.clone()));
            }
            inner.state = State::Failed(error);
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
            window_size,
            in_flight: 0,
            max_message_size: 0,
            adaptive: clock.as_ref().map(|_| AdaptiveWindow::new()),
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
        let now = self.clock.as_ref().map(|clock| clock());
        // Sending must be immediate, even if earlier calls failed or blocked.
        // Do not hold our state borrow while invoking an external message/getter.
        let _ = message.send();
        let needs_window = {
            let mut inner = self.inner.borrow_mut();
            inner.max_message_size = inner.max_message_size.max(size);
            inner.in_flight += size;
            inner.needs_window()
        };
        // C++ short-circuits the query while one largest message fits, and
        // never queries a failed stream. Besides avoiding syscalls, this avoids
        // prematurely caching an unavailable socket hint.
        let window = self
            .getter
            .as_ref()
            .filter(|_| needs_window)
            .map(|get| get());
        let (snapshot, ready) = {
            let mut inner = self.inner.borrow_mut();
            if let Some(window) = window {
                inner.window_size = window;
            }
            let ready = inner.is_ready();
            let snapshot = inner
                .adaptive
                .as_ref()
                .map(|adaptive| adaptive.sent(now.unwrap(), size, inner.window_size, !ready));
            (snapshot, ready)
        };

        let inner = self.inner.clone();
        let getter = self.getter.clone();
        let clock = self.clock.clone();
        self.tasks.add(async move {
            ack.await?;
            let now = clock.as_ref().map(|clock| clock());
            let needs_window = {
                let mut inner = inner.borrow_mut();
                inner.in_flight -= size;
                inner.needs_window()
            };
            let window = getter.as_ref().filter(|_| needs_window).map(|get| get());
            let mut inner = inner.borrow_mut();
            if let Some(window) = window {
                inner.window_size = window;
            }
            let current = inner.window_size;
            if let Some(snapshot) = snapshot {
                inner.window_size =
                    inner
                        .adaptive
                        .as_mut()
                        .unwrap()
                        .ack(snapshot, now.unwrap(), current);
            }
            inner.release_ready();
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
                    Promise::from_future(async { rcv.await.map_err(crate::canceled_to_error)? })
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
        if let State::Running(waiters) = &mut self.inner.borrow_mut().state {
            for waiter in std::mem::take(waiters) {
                let _ = waiter.send(Ok(()));
            }
        }
    }
}
