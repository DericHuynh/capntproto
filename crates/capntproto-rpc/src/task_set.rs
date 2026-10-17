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

use capntproto_rpc_engine::completion::shared as oneshot;
use futures::{Future, Stream};
use std::pin::Pin;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::task::{Context, Poll, Wake, Waker};

mod queue;
mod ready_set;
use ready_set::ReadySet;

enum EnqueuedTask<E> {
    Task(Pin<Box<dyn Future<Output = Result<(), E>>>>),
    Last(Pin<Box<dyn Future<Output = Result<(), E>>>>),
    Connection(Pin<Box<dyn Future<Output = Result<(), E>>>>),
    Terminate(Result<(), E>),
    OnEmpty(oneshot::Sender<()>),
}

struct Activity {
    ready: AtomicBool,
    parent: futures::task::AtomicWaker,
}
impl Wake for Activity {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.ready.store(true, Ordering::Release);
        self.parent.wake();
    }
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
    connection: Option<Pin<Box<dyn Future<Output = Result<(), E>>>>>,
    in_progress: ReadySet<TaskInProgress<E>>,
    last: ReadySet<TaskInProgress<E>>,
    activity: Arc<Activity>,
    activity_waker: Waker,
    last_activity: Arc<Activity>,
    last_waker: Waker,
    deferred_last: bool,
    busy_turns: u8,
    defer: fn(&mut Context<'_>),
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
        let activity = Arc::new(Activity {
            ready: AtomicBool::new(false),
            parent: futures::task::AtomicWaker::new(),
        });
        let last_activity = Arc::new(Activity {
            ready: AtomicBool::new(false),
            parent: futures::task::AtomicWaker::new(),
        });

        let mut set = Self {
            enqueued: Some(receiver),
            connection: None,
            in_progress: ReadySet::new(),
            last: ReadySet::new(),
            activity_waker: Waker::from(activity.clone()),
            activity,
            last_waker: Waker::from(last_activity.clone()),
            last_activity,
            deferred_last: false,
            busy_turns: 0,
            defer: |cx| cx.waker().wake_by_ref(),
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

    pub(crate) fn set_defer(&mut self, defer: fn(&mut Context<'_>)) {
        self.defer = defer;
    }

    fn update_on_empty_fulfillers(&mut self) {
        if self.on_empty_fulfillers.is_empty() {
            return;
        }
        // There is always the one pending() future that we added in `new()`.
        if self.connection.is_none()
            && self.in_progress.len() <= 1
            && self.last.len() == 0
            && self.enqueued.as_ref().is_none_or(|queue| queue.is_empty())
        {
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

    /// Run transport publication after ordinary ready work, allowing related
    /// messages to share one write as with KJ's evalLast().
    pub(crate) fn add_last<F>(&mut self, f: F)
    where
        F: Future<Output = Result<(), E>> + 'static,
    {
        let _ = self.sender.send(EnqueuedTask::Last(Box::pin(f)));
    }

    pub(crate) fn try_add<F>(&self, future: F) -> Result<(), ()>
    where
        F: Future<Output = Result<(), E>> + 'static,
    {
        self.sender
            .send(EnqueuedTask::Task(Box::pin(future)))
            .map_err(|_| ())
    }

    /// Poll one bounded connection directly with the parent task's waker.
    /// Additional connections retain wake-driven scheduling. This removes one
    /// notification layer without polling from application callbacks or moving
    /// a future while it is running. The connection must bound each poll.
    pub(crate) fn try_add_connection<F>(&self, future: F) -> Result<(), ()>
    where
        F: Future<Output = Result<(), E>> + 'static,
    {
        self.sender
            .send(EnqueuedTask::Connection(Box::pin(future)))
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
        self.activity.parent.register(cx.waker());
        self.last_activity.parent.register(cx.waker());
        let notified = self.activity.ready.swap(false, Ordering::AcqRel);
        let wake = self.activity_waker.clone();
        if let Poll::Ready(result) = self.poll_regular(&mut Context::from_waker(&wake)) {
            return Poll::Ready(result);
        }
        if self.last.len() == 0 || !self.last_activity.ready.load(Ordering::Acquire) {
            return Poll::Pending;
        }
        let busy = notified
            || self.activity.ready.load(Ordering::Acquire)
            || self
                .enqueued
                .as_ref()
                .is_some_and(|queue| !queue.is_empty());
        // Yield across the executor boundary too: a response consumer may be
        // an application task outside this set. Bound deferral so a perpetually
        // ready producer cannot starve transport progress.
        if (busy && self.busy_turns < 32) || !self.deferred_last {
            self.busy_turns = self.busy_turns.saturating_add(1);
            self.deferred_last = true;
            (self.defer)(cx);
            return Poll::Pending;
        }
        self.busy_turns = 0;
        self.deferred_last = false;
        self.last_activity.ready.store(false, Ordering::Release);
        let last_wake = self.last_waker.clone();
        let mut last_cx = Context::from_waker(&last_wake);
        while let Poll::Ready(Some(done)) = Pin::new(&mut self.last).poll_next(&mut last_cx) {
            match done {
                TaskDone::Continue(Ok(())) => self.reaper.task_succeeded(),
                TaskDone::Continue(Err(error)) => self.reaper.task_failed(error),
                TaskDone::Terminate(result) => return Poll::Ready(result),
            }
            self.update_on_empty_fulfillers();
        }
        if self.enqueued.is_none()
            && self.connection.is_none()
            && self.in_progress.len() <= 1
            && self.last.len() == 0
        {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }
}

impl<E: 'static> TaskSet<E> {
    fn poll_regular(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), E>> {
        let mut enqueued_stream_complete = false;
        // Run newly admitted work once before allocating a FuturesUnordered
        // node. Many RPC completions finish immediately. Pending work is then
        // polled by the set to register its own wakeup; a wake during this first
        // poll also wakes the parent, so no readiness can be lost in handoff.
        // Bound admission because a completed task/reaper may enqueue more work.
        for admission in 0..32 {
            if let Some(enqueued) = &mut self.enqueued {
                match enqueued.poll_next(cx) {
                    Poll::Pending => break,
                    Poll::Ready(None) => {
                        enqueued_stream_complete = true;
                        break;
                    }
                    Poll::Ready(Some(EnqueuedTask::Terminate(r))) => {
                        self.in_progress.push(TaskInProgress::Terminate(Some(r)));
                        break;
                    }
                    Poll::Ready(Some(EnqueuedTask::Task(mut f))) => match f.as_mut().poll(cx) {
                        Poll::Ready(result) => {
                            drop(f);
                            match result {
                                Ok(()) => self.reaper.task_succeeded(),
                                Err(error) => self.reaper.task_failed(error),
                            }
                        }
                        Poll::Pending => self.in_progress.push(TaskInProgress::Task(f)),
                    },
                    Poll::Ready(Some(EnqueuedTask::Last(f))) => {
                        self.last.push(TaskInProgress::Task(f));
                        self.last_activity.ready.store(true, Ordering::Release);
                    }
                    Poll::Ready(Some(EnqueuedTask::Connection(f))) => {
                        if self.connection.is_none() {
                            self.connection = Some(f);
                        } else {
                            self.in_progress.push(TaskInProgress::Task(f));
                        }
                    }
                    Poll::Ready(Some(EnqueuedTask::OnEmpty(f))) => {
                        self.on_empty_fulfillers.push(f);
                    }
                }
                if admission == 31 {
                    cx.waker().wake_by_ref();
                }
            } else {
                break;
            }
        }
        if enqueued_stream_complete {
            drop(self.enqueued.take());
        }

        if let Some(future) = self.connection.as_mut() {
            if let Poll::Ready(result) = future.as_mut().poll(cx) {
                let finished = self.connection.take();
                drop(finished);
                match result {
                    Ok(()) => self.reaper.task_succeeded(),
                    Err(error) => self.reaper.task_failed(error),
                }
            }
        }

        // on_empty() must also complete when it was queued on an already
        // empty set. The sentinel future is not an application task.
        self.update_on_empty_fulfillers();
        if self.enqueued.is_none()
            && self.connection.is_none()
            && self.in_progress.len() <= 1
            && self.last.len() == 0
        {
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
                        if self.enqueued.is_none()
                            && self.connection.is_none()
                            && self.in_progress.len() <= 1
                            && self.last.len() == 0
                        {
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

impl<E> Drop for TaskSet<E> {
    fn drop(&mut self) {
        drop(self.activity.parent.take());
        drop(self.last_activity.parent.take());
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
    fn last_tasks_wait_for_ready_work_and_an_external_consumer_turn() {
        let results = Rc::new(RefCell::new(Vec::new()));
        let (mut handle, mut tasks) = TaskSet::new(Box::new(Reaper(results)));
        let events = Rc::new(RefCell::new(Vec::new()));
        let seen = Rc::new(RefCell::new(Vec::new()));
        handle.add_last({
            let events = events.clone();
            let seen = seen.clone();
            async move {
                *seen.borrow_mut() = events.borrow().clone();
                Ok(())
            }
        });
        handle.add({
            let events = events.clone();
            let mut handle = handle.clone();
            async move {
                events.borrow_mut().push(1);
                handle.add(async move {
                    events.borrow_mut().push(2);
                    Ok(())
                });
                Ok(())
            }
        });
        let mut drained = handle.on_empty();
        assert!(poll(&mut tasks).is_pending());
        assert!(seen.borrow().is_empty());
        assert!(poll(&mut drained).is_pending());
        events.borrow_mut().push(3); // response consumer outside this task set
        for _ in 0..8 {
            assert!(poll(&mut tasks).is_pending());
            if !seen.borrow().is_empty() {
                break;
            }
        }
        assert_eq!(&*seen.borrow(), &[1, 2, 3]);
        assert!(matches!(poll(&mut drained), Poll::Ready(Ok(()))));
    }

    #[test]
    fn each_new_last_notification_defers_after_idle_polls() {
        use std::cell::Cell;
        let results = Rc::new(RefCell::new(Vec::new()));
        let (mut handle, mut tasks) = TaskSet::new(Box::new(Reaper(results)));
        let polls = Rc::new(Cell::new(0));
        let wake = Rc::new(RefCell::new(None));
        handle.add_last({
            let polls = polls.clone();
            let wake = wake.clone();
            std::future::poll_fn(move |cx| {
                polls.set(polls.get() + 1);
                *wake.borrow_mut() = Some(cx.waker().clone());
                Poll::Pending
            })
        });
        for _ in 0..8 {
            assert!(poll(&mut tasks).is_pending());
        }
        assert_eq!(polls.get(), 1);
        for idle_polls in 0..8 {
            for _ in 0..idle_polls {
                assert!(poll(&mut tasks).is_pending());
            }
            let before = polls.get();
            wake.borrow().as_ref().unwrap().wake_by_ref();
            assert!(poll(&mut tasks).is_pending());
            assert_eq!(polls.get(), before, "new work needs a fresh deferred turn");
            assert!(poll(&mut tasks).is_pending());
            assert_eq!(polls.get(), before + 1);
        }
    }

    #[test]
    fn perpetually_ready_work_cannot_starve_last_tasks_or_hide_their_errors() {
        let results = Rc::new(RefCell::new(Vec::new()));
        let (mut handle, mut tasks) = TaskSet::new(Box::new(Reaper(results.clone())));
        let busy = Rc::new(std::cell::Cell::new(true));
        handle.add({
            let busy = busy.clone();
            std::future::poll_fn(move |cx| {
                if busy.get() {
                    cx.waker().wake_by_ref();
                    Poll::Pending
                } else {
                    Poll::Ready(Ok(()))
                }
            })
        });
        handle.add_last(async { Err(7) });
        let mut drained = handle.on_empty();
        for _ in 0..64 {
            assert!(poll(&mut tasks).is_pending());
            if results.borrow().contains(&Err(7)) {
                break;
            }
        }
        assert_eq!(&*results.borrow(), &[Err(7)]);
        assert!(poll(&mut drained).is_pending());
        busy.set(false);
        assert!(poll(&mut tasks).is_pending());
        assert!(matches!(poll(&mut drained), Poll::Ready(Ok(()))));
    }

    #[test]
    fn direct_connection_preserves_wakes_drain_and_other_task_progress() {
        use std::{
            cell::Cell,
            sync::{
                atomic::{AtomicUsize, Ordering},
                Arc,
            },
            task::Waker,
        };
        #[derive(Default)]
        struct Notify(AtomicUsize);
        impl std::task::Wake for Notify {
            fn wake(self: Arc<Self>) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let results = Rc::new(RefCell::new(Vec::new()));
        let (mut handle, mut tasks) = TaskSet::new(Box::new(Reaper(results.clone())));
        let ready = Rc::new(Cell::new(false));
        let registered = Rc::new(RefCell::new(None::<Waker>));
        let polls = Rc::new(Cell::new(0));
        handle
            .try_add_connection({
                let ready = ready.clone();
                let registered = registered.clone();
                let polls = polls.clone();
                std::future::poll_fn(move |cx| {
                    polls.set(polls.get() + 1);
                    *registered.borrow_mut() = Some(cx.waker().clone());
                    if ready.get() {
                        Poll::Ready(Err(7))
                    } else {
                        Poll::Pending
                    }
                })
            })
            .unwrap();
        let (done, mut wait) = oneshot::channel();
        handle
            .try_add_connection(std::future::poll_fn(move |cx| {
                Pin::new(&mut wait).poll(cx).map(|_| Ok(()))
            }))
            .unwrap();
        let mut drained = handle.on_empty();
        drop(handle);
        let first_counter = Arc::new(Notify::default());
        let first = Waker::from(first_counter.clone());
        assert!(Pin::new(&mut tasks)
            .poll(&mut Context::from_waker(&first))
            .is_pending());
        assert_eq!(polls.get(), 1);
        assert!(poll(&mut drained).is_pending());
        let counter = Arc::new(Notify::default());
        let second = Waker::from(counter.clone());
        done.send(()).unwrap();
        assert!(Pin::new(&mut tasks)
            .poll(&mut Context::from_waker(&second))
            .is_pending());
        assert_eq!(polls.get(), 2);
        assert_eq!(results.borrow().as_slice(), &[Ok(())]);
        assert!(poll(&mut drained).is_pending());
        // will_wake() is only a best-effort identity check (in particular under
        // Miri). Verify delivery to the current parent and not the old parent.
        let old_previous = first_counter.0.load(Ordering::SeqCst);
        let previous = counter.0.load(Ordering::SeqCst);
        ready.set(true);
        registered.borrow().as_ref().unwrap().wake_by_ref();
        assert!(counter.0.load(Ordering::SeqCst) > previous);
        assert_eq!(first_counter.0.load(Ordering::SeqCst), old_previous);
        assert_eq!(
            Pin::new(&mut tasks).poll(&mut Context::from_waker(&second)),
            Poll::Ready(Ok(()))
        );
        assert_eq!(polls.get(), 3);
        assert_eq!(results.borrow().as_slice(), &[Ok(()), Err(7)]);
        assert_eq!(poll(&mut drained), Poll::Ready(Ok(())));
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

    #[test]
    fn admission_budget_does_not_report_empty_before_all_queued_work_runs() {
        let results = Rc::new(RefCell::new(Vec::new()));
        let (mut handle, mut tasks) = TaskSet::new(Box::new(Reaper(results.clone())));
        let mut drained = handle.on_empty();
        for _ in 0..100 {
            handle.add(async { Ok(()) });
        }
        assert!(poll(&mut tasks).is_pending());
        assert_eq!(results.borrow().len(), 31);
        assert!(poll(&mut drained).is_pending());
        drop(handle);
        for _ in 0..4 {
            if poll(&mut tasks).is_ready() {
                assert_eq!(results.borrow().len(), 100);
                assert_eq!(poll(&mut drained), Poll::Ready(Ok(())));
                return;
            }
        }
        panic!("bounded admission must eventually drain");
    }

    #[test]
    fn pending_first_poll_installs_a_live_waker_and_termination_stops_admission() {
        use std::{cell::Cell, task::Waker};
        let results = Rc::new(RefCell::new(Vec::new()));
        let (mut handle, mut tasks) = TaskSet::new(Box::new(Reaper(results.clone())));
        let ready = Rc::new(Cell::new(false));
        let wake = Rc::new(RefCell::new(None::<Waker>));
        handle.add(futures::future::poll_fn({
            let ready = ready.clone();
            let wake = wake.clone();
            move |cx| {
                if ready.get() {
                    Poll::Ready(Ok(()))
                } else {
                    *wake.borrow_mut() = Some(cx.waker().clone());
                    Poll::Pending
                }
            }
        }));
        assert!(poll(&mut tasks).is_pending());
        ready.set(true);
        wake.borrow_mut().take().unwrap().wake();
        assert!(poll(&mut tasks).is_pending());
        assert_eq!(results.borrow().as_slice(), &[Ok(())]);
        handle.terminate(Err(17));
        handle.add(async { panic!("work after termination must not run") });
        assert_eq!(poll(&mut tasks), Poll::Ready(Err(17)));
    }
}
