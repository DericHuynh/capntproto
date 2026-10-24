use super::*;

#[test]
fn scoped_control_notification_preserves_application_idle_certificates() {
    let mut notifications = Notifications::default();
    let parent = futures::task::noop_waker();
    notifications.take(&parent);
    notifications.control_waker().wake_by_ref();
    assert_eq!(notifications.take(&parent), WorkSet::CONTROL);
    assert_eq!(notifications.take(&parent), WorkSet::NONE);
    notifications.control_waker().wake_by_ref();
    notifications.enqueue().notify();
    assert_eq!(
        notifications.take(&parent),
        WorkSet::CONTROL | WorkSet::CALLS
    );
    notifications.waker().wake_by_ref();
    assert_eq!(notifications.take(&parent), WorkSet::ALL);
}

#[test]
fn every_ready_subset_is_fair_for_every_starting_lane() {
    // Independent reference: enumerate the rotated lane order, then filter it.
    // A lane which yields must get another turn only after its ready peers.
    for mask in 0..64 {
        for start in 0..6 {
            let mut schedule = Schedule {
                active: WorkSet(mask),
                cursor: Lane::ALL[start],
            };
            let expected: Vec<_> = (0..6)
                .map(|offset| Lane::ALL[(start + offset) % 6])
                .filter(|lane| mask & lane.work().0 != 0)
                .collect();
            if expected.is_empty() {
                assert_eq!(schedule.next(), None);
                continue;
            }
            for _ in 0..3 {
                for &lane in &expected {
                    assert_eq!(schedule.next(), Some(lane));
                }
            }
            for &lane in &expected {
                assert_eq!(schedule.next(), Some(lane));
                schedule.active &= !lane.work();
            }
            assert_eq!(schedule.next(), None);
        }
    }
}

#[test]
fn hot_source_cannot_restart_partial_scan_but_idle_scan_can_prefer_new_work() {
    let mut schedule = Schedule::default();
    for lane in Lane::ALL {
        schedule.resume_at(Lane::Calls);
        assert_eq!(schedule.next(), Some(lane));
        schedule.active &= !lane.work();
    }
    schedule.resume_at(Lane::Worker);
    schedule.active |= WorkSet::WORKER | WorkSet::MAINTENANCE;
    assert_eq!(schedule.next(), Some(Lane::Worker));
    schedule.resume_at(Lane::Worker);
    assert_eq!(schedule.next(), Some(Lane::Maintenance));
    assert_eq!(!WorkSet::ALL, WorkSet::NONE);
}

#[test]
fn notification_registration_replacement_and_detach_preserve_wake_ownership() {
    use std::sync::atomic::AtomicUsize;
    #[derive(Default)]
    struct Count(AtomicUsize);
    impl Wake for Count {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let mut notifications = Notifications::default();
    let first = Arc::new(Count::default());
    let second = Arc::new(Count::default());
    let a = Waker::from(first.clone());
    let b = Waker::from(second.clone());
    let enqueue = notifications.enqueue();
    // A notification before the first poll must survive registration.
    enqueue.notify();
    assert_eq!(notifications.take(&a), WorkSet::CALLS);
    assert_eq!(notifications.take(&a), WorkSet::NONE);
    assert_eq!(notifications.take(&b), WorkSet::NONE);
    notifications.waker().wake_by_ref();
    enqueue.notify();
    assert_eq!(first.0.load(Ordering::SeqCst), 0);
    assert_eq!(second.0.load(Ordering::SeqCst), 1);
    assert_eq!(notifications.take(&b), WorkSet::ALL);
    enqueue.notify();
    assert_eq!(second.0.load(Ordering::SeqCst), 2);
    assert_eq!(notifications.take(&b), WorkSet::CALLS);
    notifications.detach();
    assert!(!notifications.same_parent(&b));
    assert_eq!(Arc::strong_count(&second), 2); // external owner and b only
    enqueue.notify();
    assert_eq!(second.0.load(Ordering::SeqCst), 2);
    assert_eq!(notifications.pending(), WorkSet::CALLS);
}

#[test]
fn outgoing_notification_coalesces_without_losing_general_work() {
    use std::sync::atomic::AtomicUsize;
    struct Count(AtomicUsize);
    impl std::task::Wake for Count {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let state = Arc::<WorkWake>::default();
    let outgoing = EnqueueWake(state.clone());
    let general = Waker::from(state.clone());
    let first = Arc::new(Count(AtomicUsize::new(0)));
    let second = Arc::new(Count(AtomicUsize::new(0)));
    for outgoing_first in [true, false] {
        state.parent.register(&Waker::from(first.clone()));
        state.changed.store(0, Ordering::Release);
        let before = first.0.load(Ordering::SeqCst);
        if outgoing_first {
            outgoing.notify();
        } else {
            general.wake_by_ref();
        }
        assert_eq!(
            state.changed.load(Ordering::Acquire),
            if outgoing_first { 4 } else { 0x3f }
        );
        if outgoing_first {
            general.wake_by_ref();
        } else {
            outgoing.notify();
        }
        assert_eq!(state.changed.load(Ordering::Acquire), 0x3f);
        assert_eq!(first.0.load(Ordering::SeqCst), before + 1);
        // The consumer registers before consuming the pending mask. A later
        // outgoing-only notification must reach the new task, not the old one.
        state.parent.register(&Waker::from(second.clone()));
        assert_eq!(state.changed.swap(0, Ordering::AcqRel), 0x3f);
        outgoing.notify();
        assert_eq!(state.changed.load(Ordering::Acquire), 4);
        assert_eq!(first.0.load(Ordering::SeqCst), before + 1);
    }
    assert_eq!(second.0.load(Ordering::SeqCst), 2);
}

#[test]
fn producer_notifications_preserve_identity_across_threads_and_parent_replacement() {
    let mut notifications = Notifications::default();
    let calls = notifications.producer(WorkSet::CALLS);
    let foreign = notifications.producer(WorkSet::FOREIGN);
    let commands = notifications.producer(WorkSet::COMMANDS);
    notifications.take(Waker::noop());
    std::thread::spawn(move || {
        calls.wake_by_ref();
        calls.wake();
    })
    .join()
    .unwrap();
    assert_eq!(notifications.take(Waker::noop()), WorkSet::CALLS);
    foreign.wake();
    commands.wake();
    assert_eq!(
        notifications.take(Waker::noop()),
        WorkSet::FOREIGN | WorkSet::COMMANDS
    );
    assert_eq!(notifications.pending(), WorkSet::NONE);
}
