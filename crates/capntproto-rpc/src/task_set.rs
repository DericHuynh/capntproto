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

use alloc::sync::Arc;
use alloc::{boxed::Box, vec::Vec};
use capntproto_rpc_engine::{completion::shared as oneshot, scheduler};
use core::pin::Pin;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::Ordering;
use core::task::Context;
use core::task::Poll;
use futures::{
    task::{waker_ref, ArcWake},
    Future, Stream,
};

mod queue;
mod ready_set;
use ready_set::ReadySet;

type EnqueuedTask<E> =
    scheduler::Command<Pin<Box<dyn Future<Output = Result<(), E>>>>, E, oneshot::Sender<()>>;

struct Activity {
    ready: AtomicBool,
    parent: futures::task::AtomicWaker,
}
impl ArcWake for Activity {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        arc_self.ready.store(true, Ordering::Release);
        arc_self.parent.wake();
    }
}

enum TaskInProgress<E> {
    Task(Pin<Box<dyn Future<Output = Result<(), E>>>>),
    Terminate(Option<Result<(), E>>),
}

impl<E> Unpin for TaskInProgress<E> {}

use scheduler::Done as TaskDone;

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
    last_activity: Arc<Activity>,
    publication: scheduler::Publication,
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
        E: ::core::fmt::Debug,
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
            activity,
            last_activity,
            publication: scheduler::Publication::new(),
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
pub(crate) trait TaskReaper<E: 'static> {
    fn task_succeeded(&mut self) {}
    fn task_failed(&mut self, error: E);
}

// Keep the private hosted vtable at its allocation boundary. This statically
// dispatched provider adds no second virtual call to the engine's policy loop.
struct Reaper<'a, E: 'static>(&'a mut dyn TaskReaper<E>);
impl<E: 'static> scheduler::Reaper<E> for Reaper<'_, E> {
    #[inline(always)]
    fn task_succeeded(&mut self) {
        self.0.task_succeeded();
    }
    #[inline(always)]
    fn task_failed(&mut self, error: E) {
        self.0.task_failed(error);
    }
}

impl<E> Future for TaskSet<E>
where
    E: 'static,
{
    type Output = Result<(), E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let this = self.get_mut();
        scheduler::Parts {
            enqueued: &mut Source(&mut this.enqueued),
            connection: &mut this.connection,
            in_progress: &mut this.in_progress,
            last: &mut this.last,
            activity: scheduler::Signal {
                ready: &this.activity.ready,
                parent: &Parent(&this.activity),
            },
            last_activity: scheduler::Signal {
                ready: &this.last_activity.ready,
                parent: &Parent(&this.last_activity),
            },
            publication: &mut this.publication,
            defer: this.defer,
            on_empty_fulfillers: &mut Empty(&mut this.on_empty_fulfillers),
            reaper: &mut Reaper(&mut *this.reaper),
            idle_regular: 1,
        }
        .poll(cx)
    }
}

struct Source<'a, E>(&'a mut Option<queue::Receiver<EnqueuedTask<E>>>);
impl<E> scheduler::Source for Source<'_, E> {
    type Error = E;
    type Task = Pin<Box<dyn Future<Output = Result<(), E>>>>;
    type Empty = oneshot::Sender<()>;
    #[inline(always)]
    fn poll_next(&mut self, cx: &mut Context<'_>, _: scheduler::Capacity) -> scheduler::Next<Self> {
        let Some(source) = self.0.as_mut() else {
            return Poll::Ready(None);
        };
        source.poll_next(cx)
    }
    #[inline(always)]
    fn is_empty(&self) -> bool {
        self.0.as_ref().is_none_or(|q| q.is_empty())
    }
    #[inline(always)]
    fn is_closed(&self) -> bool {
        self.0.is_none()
    }
    #[inline(always)]
    fn close(&mut self) {
        drop(self.0.take());
    }
}
impl<E> scheduler::Work<Pin<Box<dyn Future<Output = Result<(), E>>>>, E>
    for ReadySet<TaskInProgress<E>>
{
    #[inline(always)]
    fn len(&self) -> usize {
        self.len()
    }
    #[inline(always)]
    fn has_capacity(&self) -> bool {
        true
    }
    #[inline(always)]
    fn push(&mut self, future: Pin<Box<dyn Future<Output = Result<(), E>>>>) {
        self.push(TaskInProgress::Task(future));
    }
    #[inline(always)]
    fn terminate(&mut self, result: Result<(), E>) {
        self.push(TaskInProgress::Terminate(Some(result)));
    }
    #[inline(always)]
    fn poll_next(&mut self, cx: &mut Context<'_>) -> Poll<Option<TaskDone<E>>> {
        Pin::new(self).poll_next(cx)
    }
}
struct Parent<'a>(&'a Arc<Activity>);
struct Lease(Arc<Activity>);
impl scheduler::Lease for Lease {
    #[inline(always)]
    fn waker(&self) -> impl core::ops::Deref<Target = core::task::Waker> {
        waker_ref(&self.0)
    }
}
impl scheduler::Parent for Parent<'_> {
    type Lease = Lease;
    #[inline(always)]
    fn register(&self, parent: &core::task::Waker) {
        self.0.parent.register(parent);
    }
    #[inline(always)]
    fn retain(&self) -> Lease {
        Lease(self.0.clone())
    }
}
struct Empty<'a>(&'a mut Vec<oneshot::Sender<()>>);
impl scheduler::Empty<oneshot::Sender<()>> for Empty<'_> {
    #[inline(always)]
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    #[inline(always)]
    fn has_capacity(&self) -> bool {
        true
    }
    #[inline(always)]
    fn push(&mut self, f: oneshot::Sender<()>) {
        self.0.push(f);
    }
    #[inline(always)]
    fn fulfill(&mut self) {
        for f in core::mem::take(self.0) {
            let _ = f.send(());
        }
    }
    #[inline(always)]
    fn cancel(&mut self) {
        self.0.clear();
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

    #[test]
    fn retained_connection_and_deferred_wakers_are_safe_after_task_set_drop() {
        use std::{sync::atomic::AtomicUsize, task::Waker};

        struct Notify(AtomicUsize);
        impl std::task::Wake for Notify {
            fn wake(self: Arc<Self>) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }

        for deferred in [false, true] {
            let results = Rc::new(RefCell::new(Vec::new()));
            let (mut handle, mut tasks) = TaskSet::new(Box::new(Reaper(results)));
            let registered = Rc::new(RefCell::new(None::<Waker>));
            let future = std::future::poll_fn({
                let registered = registered.clone();
                move |cx| {
                    *registered.borrow_mut() = Some(cx.waker().clone());
                    Poll::Pending
                }
            });
            if deferred {
                handle.add_last(future);
            } else {
                handle.try_add_connection(future).unwrap();
            }
            let parent = Arc::new(Notify(AtomicUsize::new(0)));
            let parent_weak = Arc::downgrade(&parent);
            let parent_waker = Waker::from(parent.clone());
            for _ in 0..3 {
                assert!(Pin::new(&mut tasks)
                    .poll(&mut Context::from_waker(&parent_waker))
                    .is_pending());
            }
            let retained = registered.borrow_mut().take().unwrap();
            let previous = parent.0.load(Ordering::SeqCst);
            retained.wake_by_ref();
            assert!(parent.0.load(Ordering::SeqCst) > previous);
            assert!(Pin::new(&mut tasks)
                .poll(&mut Context::from_waker(&parent_waker))
                .is_pending());
            drop(tasks);
            drop(handle);
            drop(parent_waker);
            drop(parent);
            assert!(parent_weak.upgrade().is_none());
            std::thread::spawn(move || {
                retained.wake_by_ref();
                retained.wake();
            })
            .join()
            .unwrap();
        }
    }
}
