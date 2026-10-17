//! Revision-backed metadata carries no capability authority or graph lifetime.
use super::*;

#[derive(Default)]
pub struct MetadataSource {
    revision: Cell<u64>,
    value: RefCell<Option<EndpointMetadata>>,
}
impl MetadataSource {
    pub fn revision(&self) -> u64 {
        self.revision.get()
    }
    pub fn snapshot(&self) -> Option<EndpointMetadata> {
        self.value.borrow().clone()
    }
    pub fn update(&self, change: impl FnOnce(&mut EndpointMetadata)) {
        let revision = self
            .revision
            .get()
            .checked_add(1)
            .expect("metadata revision exhausted");
        let mut value = self.value.borrow_mut();
        self.revision.set(revision);
        change(value.get_or_insert_with(EndpointMetadata::default));
    }
}

impl Endpoint {
    pub(super) fn apply_metadata(&self, metadata: EndpointMetadata) {
        if metadata.introduction.is_some() && self.introduction.borrow().is_none() {
            *self.introduction.borrow_mut() = metadata.introduction;
        }
        if let Some(failure) = metadata.failure {
            *self.failure.borrow_mut() = Some(failure);
        }
        #[cfg(unix)]
        if metadata.fd.is_some() && self.fd.borrow().is_none() {
            *self.fd.borrow_mut() = metadata.fd;
        }
    }
}
