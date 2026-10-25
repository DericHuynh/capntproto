use super::*;
use crate::question::{Delivery, Question};
use core::cell::{Cell, RefCell};
struct Key(u64);
struct Record {
    generation: u64,
    question: Question,
    released: bool,
}
struct Pending {
    generation: u64,
    finish: Finish,
    reclaim: bool,
}
struct Context {
    record: RefCell<Option<Record>>,
    pending: RefCell<Option<Pending>>,
    capacity: Cell<bool>,
    replace_during_finish: Cell<bool>,
    published: Cell<usize>,
    last_release_caps: Cell<bool>,
    events: RefCell<([u8; 32], usize)>,
}
impl Context {
    fn new(returned: bool) -> Self {
        let mut question = Question::new();
        if returned {
            assert_eq!(question.returned(false), Ok(Delivery::Deliver));
        }
        Self {
            record: RefCell::new(Some(Record {
                generation: 1,
                question,
                released: false,
            })),
            pending: RefCell::new(None),
            capacity: Cell::new(true),
            replace_during_finish: Cell::new(false),
            published: Cell::new(0),
            last_release_caps: Cell::new(false),
            events: RefCell::new(([0; 32], 0)),
        }
    }
    fn event(&self, event: u8) {
        let mut events = self.events.borrow_mut();
        let index = events.1;
        events.0[index] = event;
        events.1 += 1;
    }
    fn flush(&self) {
        let pending = self.pending.borrow_mut().take().unwrap();
        resume(
            self,
            &Key(pending.generation),
            pending.finish,
            pending.reclaim,
        );
    }
    fn returned(&self) {
        let mut record = self.record.borrow_mut();
        let row = record.as_mut().unwrap();
        assert_eq!(row.question.returned(false), Ok(Delivery::Discard));
        if let Some(pending) = self.pending.borrow_mut().as_mut() {
            pending.reclaim = true;
        } else {
            *record = None;
        }
    }
}
impl Owner for Context {
    type Key = Key;
    fn release(&self, key: &Key) -> Option<Release> {
        let mut record = self.record.borrow_mut();
        let row = record.as_mut()?;
        if row.generation != key.0 || row.released {
            return None;
        }
        self.event(1);
        row.released = true;
        Some(row.question.release())
    }
    fn finish(&self, key: &Key, finish: Finish, reclaim: bool) -> Publication {
        // Simulated transport entry must never run under a table borrow, and
        // must still observe the ID occupied before enqueueing Finish.
        let mut record = self
            .record
            .try_borrow_mut()
            .expect("transport reentered a borrowed table");
        let Some(row) = record.as_ref() else {
            return Publication::Accepted;
        };
        if row.generation != key.0 {
            return Publication::Accepted;
        }
        self.event(2);
        if !self.capacity.get() {
            assert!(self.pending.borrow().is_none());
            *self.pending.borrow_mut() = Some(Pending {
                generation: key.0,
                finish,
                reclaim,
            });
            return Publication::Deferred;
        }
        self.published.set(self.published.get() + 1);
        self.last_release_caps.set(finish.release_result_caps);
        if self.replace_during_finish.get() {
            *record = Some(Record {
                generation: 2,
                question: Question::new(),
                released: false,
            });
        }
        Publication::Accepted
    }
    fn reclaim(&self, key: &Key) {
        assert!(self.record.try_borrow_mut().is_ok());
        self.event(3);
        let mut record = self.record.borrow_mut();
        if record.as_ref().is_some_and(|r| r.generation == key.0) {
            assert!(self.pending.borrow().is_none());
            *record = None;
        }
    }
    fn idle(&self) {
        assert!(self.record.try_borrow_mut().is_ok());
        assert!(self.pending.try_borrow_mut().is_ok());
        self.event(4);
    }
}

#[test]
fn returned_question_publishes_before_reclaiming_with_no_allocator() {
    let measured = allocation_counter::measure(|| {
        let context = Context::new(true);
        drop(Last::new(&context, Key(1)));
        assert!(context.record.borrow().is_none());
        assert_eq!(context.published.get(), 1);
        assert!(!context.last_release_caps.get());
        assert_eq!(&context.events.borrow().0[..4], &[1, 2, 3, 4]);
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

#[test]
fn early_cancellation_keeps_the_id_until_return_without_a_second_finish() {
    let measured = allocation_counter::measure(|| {
        let context = Context::new(false);
        drop(Last::new(&context, Key(1)));
        assert!(context.record.borrow().is_some());
        assert!(context.last_release_caps.get());
        assert_eq!(&context.events.borrow().0[..3], &[1, 2, 4]);
        context.returned();
        assert!(context.record.borrow().is_none());
        assert_eq!(context.published.get(), 1);
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

#[test]
fn full_output_retains_finish_and_id_across_repeated_capacity_rejection() {
    let measured = allocation_counter::measure(|| {
        let context = Context::new(true);
        context.capacity.set(false);
        drop(Last::new(&context, Key(1)));
        assert!(context.record.borrow().is_some());
        assert!(context.pending.borrow().is_some());
        assert_eq!(context.published.get(), 0);
        run(&context, Key(1)); // A duplicate release cannot lose a pending Finish.
        assert_eq!(context.published.get(), 0);
        context.flush();
        assert!(context.record.borrow().is_some());
        assert!(context.pending.borrow().is_some());
        context.capacity.set(true);
        context.flush();
        assert!(context.record.borrow().is_none());
        assert!(context.pending.borrow().is_none());
        assert_eq!(context.published.get(), 1);
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

#[test]
fn return_during_deferred_cancellation_cannot_reuse_the_unpublished_id() {
    let measured = allocation_counter::measure(|| {
        let context = Context::new(false);
        context.capacity.set(false);
        drop(Last::new(&context, Key(1)));
        context.returned();
        assert!(context.record.borrow().is_some());
        assert!(context.pending.borrow().as_ref().unwrap().reclaim);
        context.capacity.set(true);
        context.flush();
        assert!(context.record.borrow().is_none());
        assert!(context.last_release_caps.get());
        assert_eq!(context.published.get(), 1);
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

#[test]
fn reentrant_replacement_duplicate_release_and_disarming_preserve_authority() {
    let measured = allocation_counter::measure(|| {
        let context = Context::new(true);
        context.replace_during_finish.set(true);
        drop(Last::new(&context, Key(1)));
        assert_eq!(context.record.borrow().as_ref().unwrap().generation, 2);
        run(&context, Key(1));
        assert_eq!(context.published.get(), 1);
        let key = Last::new(&context, Key(2)).disarm();
        assert_eq!(key.0, 2);
        assert!(!context.record.borrow().as_ref().unwrap().released);
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

#[test]
fn no_finish_needed_return_reclaims_without_constructing_a_finish() {
    let measured = allocation_counter::measure(|| {
        let context = Context::new(false);
        assert_eq!(
            context
                .record
                .borrow_mut()
                .as_mut()
                .unwrap()
                .question
                .returned(true),
            Ok(Delivery::Deliver)
        );
        drop(Last::new(&context, Key(1)));
        assert!(context.record.borrow().is_none());
        assert_eq!(context.published.get(), 0);
        assert_eq!(&context.events.borrow().0[..3], &[1, 3, 4]);
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}
