//! Optional read-only observations of actual record lifetimes.
//! Allocating an observer is explicit; unobserved tables allocate nothing here.
use alloc::rc::Rc;
use core::cell::Cell;

/// Live occupancy, including retained records awaiting cleanup. This handle
/// cannot access the table or keep any of its owners alive.
#[derive(Clone)]
pub struct Observation(Rc<Cell<usize>>);
impl Observation {
    pub fn get(&self) -> usize {
        self.0.get()
    }
}

#[derive(Default)]
pub(crate) struct Counter(Option<Rc<Cell<usize>>>);
#[derive(Default)]
pub(crate) struct Presence(Option<Rc<Cell<usize>>>);
impl Counter {
    pub(crate) fn enter(&self) -> Presence {
        if let Some(count) = &self.0 {
            count.set(count.get().checked_add(1).expect("live record count"));
        }
        Presence(self.0.clone())
    }
    pub(crate) fn observe<'a>(
        &mut self,
        records: impl Iterator<Item = &'a mut Presence>,
    ) -> Observation {
        if self.0.is_none() {
            self.0 = Some(Rc::new(Cell::new(0)));
            for presence in records {
                *presence = self.enter();
            }
        }
        Observation(self.0.as_ref().unwrap().clone())
    }
}
impl Drop for Presence {
    fn drop(&mut self) {
        if let Some(count) = &self.0 {
            count.set(count.get() - 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn observation_tracks_live_entries_after_counter_owner_drops() {
        let mut counter = Counter::default();
        let observation = counter.observe(core::iter::empty::<&mut Presence>());
        let entry = counter.enter();
        assert_eq!(observation.get(), 1);
        drop(counter);
        assert_eq!(observation.get(), 1);
        drop(entry);
        assert_eq!(observation.get(), 0);
    }
}
