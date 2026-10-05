// Copyright (c) 2013-2016 Sandstorm Development Group, Inc. and contributors
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
// THE SOFTWARE.

use futures::channel::oneshot;
use futures::stream::FuturesUnordered;
use futures::{Future, Stream};
use std::pin::Pin;
use std::task::{Context, Poll};

mod queue;

enum EnqueuedTask<E> {
    Task(Pin<Box<dyn Future<Output = Result<(), E>>>>),
    Terminate(Result<(), E>),
    OnEmpty(oneshot::Sender<()>),
}

enum TaskInProgress<E> {
    Task(Pin<Box<dyn Future<Output = Result<(), E>>>>),
    Terminate(Option<Result<(), E>>),
}

impl<E> Unpin for TaskInProgress<E> {}

enum TaskDone<E> {
    Continue(Result<(), E>),
    Terminate(Result<(), E>),
}

impl<E> Future for TaskInProgress<E> {
    type Output = TaskDone<E>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        match *self {
            Self::Terminate(ref mut r) => Poll::Ready(TaskDone::Terminate(r.take().unwrap())),
            Self::Task(ref mut f) => match f.as_mut().poll(cx) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(result) => Poll::Ready(TaskDone::Continue(result)),
            },
        }
    }
}

#[must_use = "a TaskSet does nothing unless polled"]
pub(crate) struct TaskSet<E> {
    enqueued: Option<queue::Receiver<EnqueuedTask<E>>>,
    in_progress: FuturesUnordered<TaskInProgress<E>>,
    on_empty_fulfillers: Vec<oneshot::Sender<()>>,
    reaper: Box<dyn TaskReaper<E>>,
}

impl<E> TaskSet<E>
where
    E: 'static,
{
    pub(crate) fn new(reaper: Box<dyn TaskReaper<E>>) -> (TaskSetHandle<E>, Self)
    where
        E: 'static,
        E: ::std::fmt::Debug,
    {
        let (sender, receiver) = queue::channel();

        let set = Self {
            enqueued: Some(receiver),
            in_progress: FuturesUnordered::new(),
            on_empty_fulfillers: vec![],
            reaper,
        };

        // If the FuturesUnordered ever gets empty, its stream will terminate, which
        // is not what we want. So we make sure there is always at least one future in it.
        set.in_progress
            .push(TaskInProgress::Task(Box::pin(::futures::future::pending())));

        let handle = TaskSetHandle { sender };

        (handle, set)
    }

    fn update_on_empty_fulfillers(&mut self) {
        // There is always the one pending() future that we added in `new()`.
        if self.in_progress.len() <= 1 {
            for f in std::mem::take(&mut self.on_empty_fulfillers) {
                let _ = f.send(());
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct TaskSetHandle<E> {
    sender: queue::Sender<EnqueuedTask<E>>,
}

impl<E> TaskSetHandle<E>
where
    E: 'static,
{
    pub(crate) fn add<F>(&mut self, f: F)
    where
        F: Future<Output = Result<(), E>> + 'static,
    {
        let _ = self.sender.send(EnqueuedTask::Task(Box::pin(f)));
    }

    pub(crate) fn try_add<F>(&self, future: F) -> Result<(), ()>
    where
        F: Future<Output = Result<(), E>> + 'static,
    {
        self.sender
            .send(EnqueuedTask::Task(Box::pin(future)))
            .map_err(|_| ())
    }

    pub(crate) fn terminate(&mut self, result: Result<(), E>) {
        let _ = self.sender.send(EnqueuedTask::Terminate(result));
    }

    /// Returns a future that finishes at the next time when the task set
    /// is empty. If the task set is terminated, the oneshot will be canceled.
    pub(crate) fn on_empty(&mut self) -> oneshot::Receiver<()> {
        let (s, r) = oneshot::channel();
        let _ = self.sender.send(EnqueuedTask::OnEmpty(s));
        r
    }
}

/// For a specific kind of task, `TaskReaper` defines the procedure that should
/// be invoked when it succeeds or fails.
pub(crate) trait TaskReaper<E>
where
    E: 'static,
{
    fn task_succeeded(&mut self) {}
    fn task_failed(&mut self, error: E);
}

impl<E> Future for TaskSet<E>
where
    E: 'static,
{
    type Output = Result<(), E>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let mut enqueued_stream_complete = false;
        if let Self {
            enqueued: Some(ref mut enqueued),
            ref mut in_progress,
            ref mut on_empty_fulfillers,
            ..
        } = self.as_mut().get_mut()
        {
            loop {
                match enqueued.poll_next(cx) {
                    Poll::Pending => break,
                    Poll::Ready(None) => {
                        enqueued_stream_complete = true;
                        break;
                    }
                    Poll::Ready(Some(EnqueuedTask::Terminate(r))) => {
                        in_progress.push(TaskInProgress::Terminate(Some(r)));
                    }
                    Poll::Ready(Some(EnqueuedTask::Task(f))) => {
                        in_progress.push(TaskInProgress::Task(f));
                    }
                    Poll::Ready(Some(EnqueuedTask::OnEmpty(f))) => {
                        on_empty_fulfillers.push(f);
                    }
                }
            }
        }
        if enqueued_stream_complete {
            drop(self.enqueued.take());
        }

        // on_empty() must also complete when it was queued on an already
        // empty set. The sentinel future is not an application task.
        self.update_on_empty_fulfillers();
        if self.enqueued.is_none() && self.in_progress.len() <= 1 {
            return Poll::Ready(Ok(()));
        }
        loop {
            match Stream::poll_next(Pin::new(&mut self.in_progress), cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(v) => match v {
                    None => return Poll::Ready(Ok(())),
                    Some(TaskDone::Continue(result)) => {
                        // FuturesUnordered has released the completed task.
                        // Reap here instead of allocating a mapped future and
                        // sharing the reaper with every task.
                        match result {
                            Ok(()) => self.reaper.task_succeeded(),
                            Err(error) => self.reaper.task_failed(error),
                        }
                        self.update_on_empty_fulfillers();
                        if self.enqueued.is_none() && self.in_progress.len() <= 1 {
                            return Poll::Ready(Ok(()));
                        }
                    }
                    Some(TaskDone::Terminate(Ok(()))) => {
                        self.on_empty_fulfillers.clear();
                        return Poll::Ready(Ok(()));
                    }
                    Some(TaskDone::Terminate(Err(e))) => {
                        self.on_empty_fulfillers.clear();
                        return Poll::Ready(Err(e));
                    }
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};

    struct Reaper(Rc<RefCell<Vec<Result<(), u8>>>>);
    impl TaskReaper<u8> for Reaper {
        fn task_succeeded(&mut self) {
            self.0.borrow_mut().push(Ok(()));
        }
        fn task_failed(&mut self, error: u8) {
            self.0.borrow_mut().push(Err(error));
        }
    }
    fn poll<F: Future + Unpin>(future: &mut F) -> Poll<F::Output> {
        Pin::new(future).poll(&mut Context::from_waker(futures::task::noop_waker_ref()))
    }

    #[test]
    fn reaps_each_result_and_waits_for_pending_work_after_last_handle_drops() {
        let results = Rc::new(RefCell::new(Vec::new()));
        let (mut handle, mut tasks) = TaskSet::new(Box::new(Reaper(results.clone())));
        let (complete, wait) = oneshot::channel();
        handle.add(async { Ok(()) });
        handle.add(async { Err(17) });
        handle.add(async { wait.await.unwrap() });
        let mut drained = handle.on_empty();
        drop(handle);
        assert!(poll(&mut tasks).is_pending());
        assert_eq!(results.borrow().as_slice(), &[Ok(()), Err(17)]);
        assert!(poll(&mut drained).is_pending());
        complete.send(Ok(())).unwrap();
        assert_eq!(poll(&mut tasks), Poll::Ready(Ok(())));
        assert_eq!(poll(&mut drained), Poll::Ready(Ok(())));
        assert_eq!(results.borrow().as_slice(), &[Ok(()), Err(17), Ok(())]);
    }

    #[test]
    fn termination_cancels_drain_and_drop_rejects_new_work() {
        let results = Rc::new(RefCell::new(Vec::new()));
        let (mut handle, mut tasks) = TaskSet::new(Box::new(Reaper(results.clone())));
        let (complete, wait) = oneshot::channel::<Result<(), u8>>();
        handle.add(async { wait.await.unwrap() });
        let mut drained = handle.on_empty();
        assert!(poll(&mut tasks).is_pending());
        handle.terminate(Err(23));
        assert_eq!(poll(&mut tasks), Poll::Ready(Err(23)));
        assert!(matches!(poll(&mut drained), Poll::Ready(Err(_))));
        drop(tasks);
        assert!(complete.send(Ok(())).is_err());
        assert!(handle.try_add(async { Ok(()) }).is_err());
        assert!(results.borrow().is_empty(), "canceled tasks are not reaped");
    }

    #[test]
    fn a_reaper_can_enqueue_followup_work_without_borrowing_a_running_task() {
        struct Followup {
            handle: Rc<RefCell<Option<TaskSetHandle<u8>>>>,
            errors: Rc<RefCell<Vec<u8>>>,
        }
        impl TaskReaper<u8> for Followup {
            fn task_failed(&mut self, error: u8) {
                self.errors.borrow_mut().push(error);
                if error == 1 {
                    self.handle
                        .borrow_mut()
                        .as_mut()
                        .unwrap()
                        .add(async { Err(2) });
                }
            }
        }
        let shared = Rc::new(RefCell::new(None));
        let errors = Rc::new(RefCell::new(Vec::new()));
        let (mut handle, mut tasks) = TaskSet::new(Box::new(Followup {
            handle: shared.clone(),
            errors: errors.clone(),
        }));
        *shared.borrow_mut() = Some(handle.clone());
        handle.add(async { Err(1) });
        assert!(poll(&mut tasks).is_pending());
        assert!(poll(&mut tasks).is_pending());
        assert_eq!(errors.borrow().as_slice(), &[1, 2]);
        shared.borrow_mut().take();
        drop(handle);
        assert_eq!(poll(&mut tasks), Poll::Ready(Ok(())));
    }
}
