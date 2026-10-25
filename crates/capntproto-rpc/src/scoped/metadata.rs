//! Change subscriptions carry no capability authority or graph lifetime.
use super::*;

#[derive(Default)]
pub struct MetadataSource {
    value: RefCell<Option<EndpointMetadata>>,
    subscribers: RefCell<Vec<Subscription>>,
}
#[derive(Clone)]
struct Subscription {
    endpoint: Weak<Endpoint>,
    generation: u64,
}
impl Subscription {
    fn endpoint(&self) -> Option<Rc<Endpoint>> {
        self.endpoint
            .upgrade()
            .filter(|endpoint| endpoint.metadata_generation.get() == self.generation)
    }
}
impl MetadataSource {
    pub(super) fn subscribe(&self, endpoint: &Rc<Endpoint>) {
        let generation = endpoint.next_metadata_generation();
        let mut subscribers = self.subscribers.borrow_mut();
        subscribers.retain(|entry| entry.endpoint().is_some());
        subscribers.push(Subscription {
            endpoint: Rc::downgrade(endpoint),
            generation,
        });
    }
    pub fn snapshot(&self) -> Option<EndpointMetadata> {
        self.value.borrow().clone()
    }
    pub fn update(&self, change: impl FnOnce(&mut EndpointMetadata)) {
        let mut value = self.value.borrow_mut();
        change(value.get_or_insert_with(EndpointMetadata::default));
        let snapshot = value.clone().expect("updated metadata");
        drop(value);
        // Publish outside the registry borrow: opaque metadata subscribers can
        // wake an executor which queries, drops, or updates this source again.
        let subscribers = {
            let mut entries = self.subscribers.borrow_mut();
            entries.retain(|entry| entry.endpoint().is_some());
            entries.clone()
        };
        for subscriber in subscribers {
            if let Some(endpoint) = subscriber.endpoint() {
                endpoint.apply_metadata(snapshot.clone());
            }
        }
    }
}

impl Endpoint {
    pub(super) fn next_metadata_generation(&self) -> u64 {
        let generation = self
            .metadata_generation
            .get()
            .checked_add(1)
            .expect("metadata subscription generation exhausted");
        self.metadata_generation.set(generation);
        generation
    }
    pub(super) fn apply_metadata(&self, metadata: EndpointMetadata) {
        let mut changed = false;
        if metadata.introduction.is_some() && self.introduction.borrow().is_none() {
            *self.introduction.borrow_mut() = metadata.introduction;
            changed = true;
        }
        if let Some(failure) = metadata.failure {
            *self.failure.borrow_mut() = Some(failure);
            changed = true;
        }
        #[cfg(unix)]
        if metadata.fd.is_some() && self.fd.borrow().is_none() {
            *self.fd.borrow_mut() = metadata.fd;
            changed = true;
        }
        if changed {
            if let Some(source) = self.metadata_changes.get() {
                source.notify();
            }
        }
    }
}
