//! Executor adapters for the private route-owner boundary. Abort requests stay
//! synchronous; captured futures are released on the next executor poll.
use super::LocalExecutor;
use capntproto_test_support::schedules::{spawn_local, JoinHandle};
use std::{cell::RefCell, future::Future, pin::Pin, rc::Rc, task::Poll};

type Work = Pin<Box<dyn Future<Output = Result<(), futures::future::Aborted>>>>;
pub(super) struct Task {
    work: Option<Work>,
    pub(super) abort: futures::future::AbortHandle,
    pub(super) state: u64,
}
#[derive(Clone, Default)]
pub(super) struct Manual(pub(super) Rc<RefCell<Vec<Task>>>);
impl LocalExecutor for Manual {
    type Abort = futures::future::AbortHandle;
    fn spawn(&self, work: impl Future<Output = ()> + 'static) -> Self::Abort {
        let (abort, registration) = futures::future::AbortHandle::new_pair();
        self.0.borrow_mut().push(Task {
            work: Some(Box::pin(futures::future::Abortable::new(
                work,
                registration,
            ))),
            abort: abort.clone(),
            state: 0,
        });
        abort
    }
    fn abort(task: &Self::Abort) {
        task.abort();
    }
}
impl Manual {
    pub(super) fn poll(&self, id: usize) {
        let mut tasks = self.0.borrow_mut();
        let task = &mut tasks[id];
        let result =
            task.work
                .as_mut()
                .unwrap()
                .as_mut()
                .poll(&mut std::task::Context::from_waker(
                    futures::task::noop_waker_ref(),
                ));
        task.state = match result {
            Poll::Pending => 1,
            Poll::Ready(Ok(())) => 2,
            Poll::Ready(Err(_)) => 3,
        };
        if result.is_ready() {
            task.work.take();
        }
    }
}

#[derive(Clone, Default)]
pub(super) struct Executor(Rc<RefCell<Vec<JoinHandle<()>>>>);
impl LocalExecutor for Executor {
    type Abort = futures::future::AbortHandle;
    fn spawn(&self, work: impl Future<Output = ()> + 'static) -> Self::Abort {
        // Shuttle's own abort handle schedules inside abort(), unlike Tokio's
        // local abort. Abortable keeps this operation indivisible.
        let (abort, registration) = futures::future::AbortHandle::new_pair();
        let task = spawn_local(async move {
            let _ = futures::future::Abortable::new(work, registration).await;
        });
        self.0.borrow_mut().push(task);
        abort
    }
    fn abort(task: &Self::Abort) {
        task.abort();
    }
}
impl Executor {
    pub(super) async fn join(&self) {
        let tasks = self.0.take();
        for task in tasks {
            task.await.unwrap();
        }
    }
}
