extern crate std;
use super::*;
use crate::{admission, completion, readiness, tasks};
use core::{
    cell::{Cell, RefCell},
    marker::PhantomPinned,
};

struct Env {
    regular: &'static readiness::Storage<1>,
    deferred: &'static readiness::Storage<1>,
    ready: &'static AtomicBool,
    last_ready: &'static AtomicBool,
    wakes: [Waker; 1],
    last_wakes: [Waker; 1],
    outer: Waker,
    parent: Waker,
    last_parent: Waker,
    notifications: &'static core::sync::atomic::AtomicUsize,
}
macro_rules! environment {
    () => {{
        static REGULAR: readiness::Storage<1> = readiness::Storage::new();
        static DEFERRED: readiness::Storage<1> = readiness::Storage::new();
        static READY: AtomicBool = AtomicBool::new(false);
        static LAST_READY: AtomicBool = AtomicBool::new(false);
        static NOTIFICATIONS: core::sync::atomic::AtomicUsize =
            core::sync::atomic::AtomicUsize::new(0);
        fn outer() {
            NOTIFICATIONS.fetch_add(1, Ordering::Relaxed);
        }
        fn regular() {
            READY.store(true, Ordering::Release);
            NOTIFICATIONS.fetch_add(1, Ordering::Relaxed);
        }
        fn deferred() {
            LAST_READY.store(true, Ordering::Release);
            NOTIFICATIONS.fetch_add(1, Ordering::Relaxed);
        }
        static WAKES: [tasks::Signal<1>; 1] = tasks::signals(&REGULAR, regular);
        static LAST_WAKES: [tasks::Signal<1>; 1] = tasks::signals(&DEFERRED, deferred);
        struct Parent(fn());
        impl capntproto_task::Wake for Parent {
            fn wake_by_ref(&self) {
                (self.0)();
            }
        }
        static OUTER: capntproto_task::Static<Parent> = capntproto_task::Static::new(Parent(outer));
        static PARENT: capntproto_task::Static<Parent> =
            capntproto_task::Static::new(Parent(regular));
        static LAST_PARENT: capntproto_task::Static<Parent> =
            capntproto_task::Static::new(Parent(deferred));
        Env {
            regular: &REGULAR,
            deferred: &DEFERRED,
            ready: &READY,
            last_ready: &LAST_READY,
            wakes: [WAKES[0].waker()],
            last_wakes: [LAST_WAKES[0].waker()],
            outer: OUTER.waker(),
            parent: PARENT.waker(),
            last_parent: LAST_PARENT.waker(),
            notifications: &NOTIFICATIONS,
        }
    }};
}
macro_rules! scheduler {
    ($env:ident, $inbox:ident, $connection:ident, $regular:ident, $last:ident, $empty:ident, $reaper:ident, $deferred:ident, $busy:ident) => {
        Parts {
            enqueued: &mut $inbox,
            connection: &mut $connection,
            in_progress: &mut $regular,
            last: &mut $last,
            activity: Signal {
                ready: $env.ready,
                parent: &bounded::StaticParent::new(&$env.parent, |_| {}),
            },
            last_activity: Signal {
                ready: $env.last_ready,
                parent: &bounded::StaticParent::new(&$env.last_parent, |_| {}),
            },
            deferred_last: &mut $deferred,
            busy_turns: &mut $busy,
            defer: |cx| cx.waker().wake_by_ref(),
            on_empty_fulfillers: &mut $empty,
            reaper: &mut $reaper,
            idle_regular: 0,
        }
    };
}
struct State {
    polls: [Cell<usize>; 64],
    drops: [Cell<usize>; 64],
    ready: [Cell<bool>; 64],
    wakes: [RefCell<Option<Waker>>; 64],
    events: RefCell<([usize; 128], usize)>,
}
impl State {
    fn new() -> Self {
        Self {
            polls: [const { Cell::new(0) }; 64],
            drops: [const { Cell::new(0) }; 64],
            ready: [const { Cell::new(false) }; 64],
            wakes: [const { RefCell::new(None) }; 64],
            events: RefCell::new(([0; 128], 0)),
        }
    }
    fn event(&self, value: usize) {
        let mut log = self.events.borrow_mut();
        let index = log.1;
        log.0[index] = value;
        log.1 += 1;
    }
    fn job(&self, index: usize, self_wake: bool) -> Job<'_> {
        Job {
            index,
            state: self,
            address: Cell::new(0),
            self_wake,
            _pin: PhantomPinned,
        }
    }
    fn finish(&self, index: usize) {
        self.ready[index].set(true);
        self.wakes[index].borrow_mut().take().unwrap().wake();
    }
}
struct Job<'a> {
    index: usize,
    state: &'a State,
    address: Cell<usize>,
    self_wake: bool,
    _pin: PhantomPinned,
}
impl Future for Job<'_> {
    type Output = Result<(), u8>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let address = core::ptr::from_ref(self.as_ref().get_ref()).addr();
        let previous = self.address.replace(address);
        assert!(previous == 0 || previous == address);
        let polls = &self.state.polls[self.index];
        if polls.get() == 0 {
            self.state.event(self.index);
        }
        polls.set(polls.get() + 1);
        if self.state.ready[self.index].get() {
            Poll::Ready(Ok(()))
        } else {
            *self.state.wakes[self.index].borrow_mut() = Some(cx.waker().clone());
            if self.self_wake {
                cx.waker().wake_by_ref();
            }
            Poll::Pending
        }
    }
}
impl Drop for Job<'_> {
    fn drop(&mut self) {
        if self.address.get() != 0 {
            assert_eq!(self.address.get(), core::ptr::from_ref(self).addr());
        }
        self.state.drops[self.index].set(self.state.drops[self.index].get() + 1);
    }
}
#[derive(Default)]
struct Reaped {
    succeeded: usize,
    failed: usize,
}
impl Reaper<u8> for Reaped {
    fn task_succeeded(&mut self) {
        self.succeeded += 1;
    }
    fn task_failed(&mut self, _: u8) {
        self.failed += 1;
    }
}

#[test]
fn capacity_precedes_first_poll_and_released_slots_wake_blocked_admission() {
    let env = environment!();
    let measured = allocation_counter::measure(|| {
        let state = State::new();
        state.ready[1].set(true);
        state.ready[2].set(true);
        let mut owners = core::pin::pin!(capntproto_task::Slots::<_, 3>::new());
        let places = owners.as_mut().places();
        let mut stop = completion::Slot::new();
        let (_stopper, stopped) = stop.pair().unwrap();
        let mut notification = completion::Slot::new();
        let (notify, mut notified) = notification.pair().unwrap();
        let mut channel = admission::Slot::<_, 4>::new();
        let (sender, receiver) = channel.pair();
        for (index, place) in places.into_iter().enumerate() {
            assert!(sender
                .send(Command::Task(
                    place.insert_erased(state.job(index, false)).ok().unwrap()
                ))
                .is_ok());
        }
        assert!(sender.send(Command::OnEmpty(notify)).is_ok());
        let mut inbox = bounded::Inbox::new(receiver, stopped);
        let mut reg_slots = core::pin::pin!(tasks::PinnedSlots::new());
        let mut last_slots = core::pin::pin!(tasks::PinnedSlots::new());
        let (mut reg_cursor, mut last_cursor) = (0, 0);
        let mut regular = bounded::Tasks::new(tasks::Set::new(
            reg_slots.as_mut(),
            env.regular.queue(),
            &env.wakes,
            &mut reg_cursor,
        ));
        let mut last = bounded::Tasks::new(tasks::Set::new(
            last_slots.as_mut(),
            env.deferred.queue(),
            &env.last_wakes,
            &mut last_cursor,
        ));
        let mut empty = bounded::Notifications::<1>::new();
        let mut reaper = Reaped::default();
        let mut connection = None;
        let mut deferred = false;
        let mut busy = 0;
        let mut runtime =
            scheduler!(env, inbox, connection, regular, last, empty, reaper, deferred, busy);
        let mut cx = Context::from_waker(&env.outer);
        assert!(runtime.poll(&mut cx).is_pending());
        assert_eq!(state.polls[1].get(), 0);
        assert_eq!(state.polls[2].get(), 0);
        state.finish(0);
        let wakes = env.notifications.load(Ordering::Relaxed);
        assert!(runtime.poll(&mut cx).is_pending());
        assert!(env.notifications.load(Ordering::Relaxed) > wakes);
        assert_eq!(state.drops[0].get(), 1);
        assert!(runtime.poll(&mut cx).is_pending());
        assert_eq!(Pin::new(&mut notified).poll(&mut cx), Poll::Ready(Ok(())));
        assert_eq!(runtime.reaper.succeeded, 3);
        assert!(state.drops[..3].iter().all(|v| v.get() == 1));
        drop(sender);
        assert_eq!(runtime.poll(&mut cx), Poll::Ready(Ok(())));
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

#[test]
fn independent_stop_bypasses_saturation_and_cancels_empty_notifications() {
    let env = environment!();
    let measured = allocation_counter::measure(|| {
        let state = State::new();
        let mut owners = core::pin::pin!(capntproto_task::Slots::<_, 3>::new());
        {
            let places = owners.as_mut().places();
            let mut stop = completion::Slot::new();
            let (stopper, stopped) = stop.pair().unwrap();
            let mut notification = completion::Slot::new();
            let (notify, mut notified) = notification.pair().unwrap();
            let mut channel = admission::Slot::<_, 4>::new();
            let (sender, receiver) = channel.pair();
            assert!(sender.send(Command::OnEmpty(notify)).is_ok());
            for (index, place) in places.into_iter().enumerate() {
                assert!(sender
                    .send(Command::Task(
                        place.insert(state.job(index, false)).ok().unwrap()
                    ))
                    .is_ok());
            }
            let mut inbox = bounded::Inbox::new(receiver, stopped);
            let mut reg_slots = core::pin::pin!(tasks::PinnedSlots::new());
            let mut last_slots = core::pin::pin!(tasks::PinnedSlots::new());
            let (mut reg_cursor, mut last_cursor) = (0, 0);
            let mut regular = bounded::Tasks::new(tasks::Set::new(
                reg_slots.as_mut(),
                env.regular.queue(),
                &env.wakes,
                &mut reg_cursor,
            ));
            let mut last = bounded::Tasks::new(tasks::Set::new(
                last_slots.as_mut(),
                env.deferred.queue(),
                &env.last_wakes,
                &mut last_cursor,
            ));
            let mut empty = bounded::Notifications::<1>::new();
            let mut reaper = Reaped::default();
            let mut connection = None;
            let mut deferred = false;
            let mut busy = 0;
            let mut runtime =
                scheduler!(env, inbox, connection, regular, last, empty, reaper, deferred, busy);
            let mut cx = Context::from_waker(&env.outer);
            assert!(runtime.poll(&mut cx).is_pending());
            assert_eq!(state.polls[1].get(), 0);
            let wakes = env.notifications.load(Ordering::Relaxed);
            assert!(stopper.send(Err(23)).is_ok());
            assert!(env.notifications.load(Ordering::Relaxed) > wakes);
            assert_eq!(runtime.poll(&mut cx), Poll::Ready(Err(23)));
            assert_eq!(
                Pin::new(&mut notified).poll(&mut cx),
                Poll::Ready(Err(completion::Error::Canceled))
            );
            assert_eq!(state.polls[2].get(), 0);
        }
        assert!(owners.is_empty());
        assert!(state.drops[..3].iter().all(|v| v.get() == 1));
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

#[test]
fn direct_connection_and_deferred_publication_preserve_external_consumer_turn() {
    let env = environment!();
    let measured = allocation_counter::measure(|| {
        let state = State::new();
        for index in [0, 2, 3] {
            state.ready[index].set(true);
        }
        let mut owners = core::pin::pin!(capntproto_task::Slots::<_, 4>::new());
        let [a, b, c, d] = owners.as_mut().places();
        let mut stop = completion::Slot::new();
        let (_stopper, stopped) = stop.pair().unwrap();
        let mut notification = completion::Slot::new();
        let (notify, mut notified) = notification.pair().unwrap();
        let mut channel = admission::Slot::<_, 8>::new();
        let (sender, receiver) = channel.pair();
        assert!(sender
            .send(Command::Task(a.insert(state.job(0, false)).ok().unwrap()))
            .is_ok());
        assert!(sender
            .send(Command::Last(c.insert(state.job(2, false)).ok().unwrap()))
            .is_ok());
        assert!(sender
            .send(Command::Connection(
                b.insert(state.job(1, false)).ok().unwrap()
            ))
            .is_ok());
        assert!(sender
            .send(Command::Connection(
                d.insert(state.job(3, false)).ok().unwrap()
            ))
            .is_ok());
        assert!(sender.send(Command::OnEmpty(notify)).is_ok());
        let mut inbox = bounded::Inbox::new(receiver, stopped);
        let mut reg_slots = core::pin::pin!(tasks::PinnedSlots::new());
        let mut last_slots = core::pin::pin!(tasks::PinnedSlots::new());
        let (mut reg_cursor, mut last_cursor) = (0, 0);
        let mut regular = bounded::Tasks::new(tasks::Set::new(
            reg_slots.as_mut(),
            env.regular.queue(),
            &env.wakes,
            &mut reg_cursor,
        ));
        let mut last = bounded::Tasks::new(tasks::Set::new(
            last_slots.as_mut(),
            env.deferred.queue(),
            &env.last_wakes,
            &mut last_cursor,
        ));
        let mut empty = bounded::Notifications::<1>::new();
        let mut reaper = Reaped::default();
        let mut connection = None;
        let mut deferred = false;
        let mut busy = 0;
        let mut runtime =
            scheduler!(env, inbox, connection, regular, last, empty, reaper, deferred, busy);
        let mut cx = Context::from_waker(&env.outer);
        assert!(runtime.poll(&mut cx).is_pending());
        assert_eq!(state.polls[2].get(), 0);
        assert!(Pin::new(&mut notified).poll(&mut cx).is_pending());
        state.event(9);
        assert!(runtime.poll(&mut cx).is_pending());
        assert_eq!(state.drops[2].get(), 1);
        assert_eq!(&state.events.borrow().0[..5], &[0, 1, 3, 9, 2]);
        assert!(Pin::new(&mut notified).poll(&mut cx).is_pending());
        state.finish(1);
        assert!(runtime.poll(&mut cx).is_pending());
        assert_eq!(state.drops[1].get(), 1);
        assert_eq!(Pin::new(&mut notified).poll(&mut cx), Poll::Ready(Ok(())));
        drop(sender);
        assert_eq!(runtime.poll(&mut cx), Poll::Ready(Ok(())));
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

#[test]
fn perpetually_ready_ordinary_work_cannot_starve_deferred_io() {
    let env = environment!();
    let measured = allocation_counter::measure(|| {
        let state = State::new();
        state.ready[1].set(true);
        let mut owners = core::pin::pin!(capntproto_task::Slots::<_, 2>::new());
        let [a, b] = owners.as_mut().places();
        let mut stop = completion::Slot::new();
        let (_stopper, stopped) = stop.pair().unwrap();
        let mut channel = admission::Slot::<_, 2>::new();
        let (sender, receiver) = channel.pair();
        assert!(sender
            .send(Command::Task(a.insert(state.job(0, true)).ok().unwrap()))
            .is_ok());
        assert!(sender
            .send(Command::Last(b.insert(state.job(1, false)).ok().unwrap()))
            .is_ok());
        let mut inbox = bounded::Inbox::new(receiver, stopped);
        let mut reg_slots = core::pin::pin!(tasks::PinnedSlots::new());
        let mut last_slots = core::pin::pin!(tasks::PinnedSlots::new());
        let (mut reg_cursor, mut last_cursor) = (0, 0);
        let mut regular = bounded::Tasks::new(tasks::Set::new(
            reg_slots.as_mut(),
            env.regular.queue(),
            &env.wakes,
            &mut reg_cursor,
        ));
        let mut last = bounded::Tasks::new(tasks::Set::new(
            last_slots.as_mut(),
            env.deferred.queue(),
            &env.last_wakes,
            &mut last_cursor,
        ));
        let mut empty = bounded::Notifications::<0>::new();
        let mut reaper = Reaped::default();
        let mut connection = None;
        let mut deferred = false;
        let mut busy = 0;
        let mut runtime =
            scheduler!(env, inbox, connection, regular, last, empty, reaper, deferred, busy);
        let mut cx = Context::from_waker(&env.outer);
        for _ in 0..34 {
            assert!(runtime.poll(&mut cx).is_pending());
            if state.drops[1].get() != 0 {
                break;
            }
        }
        assert_eq!(state.drops[1].get(), 1);
        assert!(state.polls[0].get() < 40);
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}
