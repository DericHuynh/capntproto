//! Explicit change notifications for opaque hook metadata. The hook still
//! decides what diagnostics and resources its policy permits callers to see.
use alloc::{
    rc::{Rc, Weak},
    vec::Vec,
};
use core::{
    cell::{Cell, RefCell},
    task::{Context, Poll, Waker},
};

#[derive(Default)]
pub struct MetadataChanges {
    subscribers: RefCell<Vec<Weak<State>>>,
    parents: Vec<Rc<MetadataChanges>>,
}
struct State {
    changed: Cell<bool>,
    waker: RefCell<Option<Waker>>,
}
pub struct MetadataWatch {
    source: Rc<MetadataChanges>,
    state: Rc<State>,
}
impl MetadataChanges {
    /// Combine immutable notification sources without querying metadata or
    /// scheduling another task. Parents predate this source, so the topology
    /// cannot contain a strong cycle. Policies still own all metadata queries.
    pub fn combined(parents: Vec<Rc<MetadataChanges>>) -> Self {
        Self {
            subscribers: RefCell::new(Vec::new()),
            parents,
        }
    }
    fn attach(&self, state: &Rc<State>) {
        for parent in &self.parents {
            parent.attach(state);
        }
        let mut subscribers = self.subscribers.borrow_mut();
        subscribers.retain(|entry| entry.strong_count() != 0);
        subscribers.push(Rc::downgrade(state));
    }
    pub fn subscribe(self: &Rc<Self>) -> MetadataWatch {
        let state = Rc::new(State {
            changed: Cell::new(false),
            waker: RefCell::new(None),
        });
        self.attach(&state);
        MetadataWatch {
            source: self.clone(),
            state,
        }
    }
    /// Publish after updating metadata. Wake outside all source/state borrows:
    /// an executor is allowed to query metadata, unsubscribe, or notify again.
    pub fn notify(&self) {
        let subscribers: Vec<_> = {
            let mut entries = self.subscribers.borrow_mut();
            entries.retain(|entry| entry.strong_count() != 0);
            entries.iter().filter_map(Weak::upgrade).collect()
        };
        for state in subscribers {
            state.changed.set(true);
            let wake = state.waker.borrow_mut().take();
            if let Some(wake) = wake {
                wake.wake();
            }
        }
    }
}
impl MetadataWatch {
    pub fn source(&self) -> &Rc<MetadataChanges> {
        &self.source
    }
    pub fn poll_changed(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        let replace = !self
            .state
            .waker
            .borrow()
            .as_ref()
            .is_some_and(|w| w.will_wake(cx.waker()));
        if replace {
            let wake = cx.waker().clone();
            let previous = self.state.waker.replace(Some(wake));
            drop(previous);
        }
        if self.state.changed.replace(false) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    struct Count(AtomicUsize);
    impl std::task::Wake for Count {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    #[test]
    fn sources_coalesce_wakes_and_dropped_subscriptions_stop_observing() {
        let a = Rc::new(MetadataChanges::default());
        let b = Rc::new(MetadataChanges::default());
        let combined = Rc::new(MetadataChanges::combined(alloc::vec![a.clone(), b.clone()]));
        let wake = Arc::new(Count(AtomicUsize::new(0)));
        let waker = Waker::from(wake.clone());
        let mut cx = Context::from_waker(&waker);
        let mut watch = combined.subscribe();
        assert!(watch.poll_changed(&mut cx).is_pending());
        a.notify();
        b.notify();
        assert_eq!(wake.0.load(Ordering::SeqCst), 1);
        assert!(watch.poll_changed(&mut cx).is_ready());
        assert!(watch.poll_changed(&mut cx).is_pending());
        combined.notify();
        assert!(watch.poll_changed(&mut cx).is_ready());
        drop(watch);
        let before = wake.0.load(Ordering::SeqCst);
        a.notify();
        b.notify();
        combined.notify();
        assert_eq!(wake.0.load(Ordering::SeqCst), before);
    }
}
