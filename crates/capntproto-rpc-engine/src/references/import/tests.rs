use super::*;
use core::cell::{Cell, RefCell};

struct Key(u32);
struct Context {
    active: RefCell<Option<u32>>,
    pending: RefCell<Option<(u32, u32)>>,
    capacity: Cell<bool>,
    terminal: Cell<bool>,
    sent: Cell<u32>,
    events: RefCell<([u8; 3], usize)>,
}
impl Context {
    fn new(capacity: bool) -> Self {
        Self {
            active: RefCell::new(Some(7)),
            pending: RefCell::new(None),
            capacity: Cell::new(capacity),
            terminal: Cell::new(false),
            sent: Cell::new(0),
            events: RefCell::new(([0; 3], 0)),
        }
    }
    fn event(&self, event: u8) {
        let mut events = self.events.borrow_mut();
        let index = events.1;
        events.0[index] = event;
        events.1 += 1;
    }
}
impl Owner for Context {
    type Key = Key;
    fn retire(&self, key: &Key) {
        assert_eq!(self.active.borrow_mut().take(), Some(key.0));
        self.event(1);
    }
    fn release(&self, key: &Key, references: &mut ImportRefs) {
        assert!(self.active.try_borrow_mut().unwrap().is_none());
        self.event(2);
        if self.terminal.get() {
            return;
        }
        let Some(ticket) = references.pending_release() else {
            return;
        };
        if self.capacity.get() {
            self.sent.set(ticket.count());
        } else {
            assert!(self
                .pending
                .borrow_mut()
                .replace((key.0, ticket.count()))
                .is_none());
        }
        ticket.commit();
    }
    fn idle(&self) {
        assert!(self.active.try_borrow_mut().unwrap().is_none());
        assert!(self.pending.try_borrow_mut().is_ok());
        self.event(3);
    }
}
fn references(count: u32) -> ImportRefs {
    let mut references = ImportRefs::new();
    references.receive(count).unwrap();
    references
}

#[test]
fn borrowed_last_owner_releases_all_wire_references_after_retirement() {
    let context = Context::new(true);
    let mut last = Last::new(&context, Key(7), references(3));
    assert!(core::ptr::eq(last.owner(), &context));
    last.references().receive(2).unwrap();
    assert_eq!(context.sent.get(), 0);
    drop(last);
    assert_eq!(context.sent.get(), 5);
    assert_eq!(*context.events.borrow(), ([1, 2, 3], 3));
}

#[test]
fn backpressured_release_survives_the_last_local_owner() {
    let context = Context::new(false);
    drop(Last::new(&context, Key(7), references(3)));
    assert_eq!(context.sent.get(), 0);
    assert!(context.active.borrow().is_none());
    assert_eq!(*context.pending.borrow(), Some((7, 3)));
    context.capacity.set(true);
    let (key, count) = context.pending.borrow_mut().take().unwrap();
    assert_eq!(key, 7);
    context.sent.set(count);
    assert!(context.pending.borrow().is_none());
    assert_eq!(context.sent.get(), 3);
    assert_eq!(*context.events.borrow(), ([1, 2, 3], 3));
}

#[test]
fn transferring_obligation_does_not_retire_or_publish_it() {
    let context = Context::new(true);
    let (key, references) = Last::new(&context, Key(7), references(9)).into_parts();
    assert_eq!(*context.active.borrow(), Some(7));
    assert_eq!(references.count(), 9);
    assert_eq!(context.events.borrow().1, 0);
    drop(Last::new(&context, key, references));
    assert_eq!(context.sent.get(), 9);
}

#[test]
fn empty_or_terminal_imports_retire_without_a_release_message() {
    for terminal in [false, true] {
        let context = Context::new(true);
        context.terminal.set(terminal);
        drop(Last::new(
            &context,
            Key(7),
            references(if terminal { 3 } else { 0 }),
        ));
        assert_eq!(context.sent.get(), 0);
        assert!(context.pending.borrow().is_none());
        assert_eq!(*context.events.borrow(), ([1, 2, 3], 3));
    }
}
