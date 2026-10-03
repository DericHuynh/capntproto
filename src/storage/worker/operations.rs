//! Keep the public command surface typed and its memory accounting explicit.
use super::*;
use crate::storage::{ComponentPublication, Publication, PublicationCursor};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectStatus {
    pub head: Revision,
    pub published: Revision,
    pub history_floor: Revision,
}

impl Client {
    pub fn try_put(
        &self,
        object: ObjectKey,
        expected_head: Revision,
        value: &[u8],
        options: Options,
    ) -> Result<Pending<Revision>, AdmissionError> {
        self.submit(value.len(), options, || {
            let value = value.to_vec();
            move |store| store.put(object, expected_head, &value)
        })
    }

    /// One atomic V4-style batch, including when stored in a V5 file. CAS is
    /// checked during execution; successful admission is not a commit receipt.
    pub fn try_commit(
        &self,
        updates: &[Update<'_>],
        options: Options,
    ) -> Result<Pending<Vec<Revision>>, AdmissionError> {
        if updates.is_empty() || updates.len() > 16 {
            return Err(self.rejection(AdmissionError::InvalidRequest));
        }
        let bytes = updates
            .iter()
            .try_fold(0usize, |n, u| n.checked_add(u.value.len()))
            .ok_or_else(|| self.rejection(AdmissionError::TooLarge))?;
        self.submit(bytes, options, || {
            let updates: Vec<_> = updates
                .iter()
                .map(|u| {
                    (
                        u.object,
                        u.expected_head,
                        u.expected_published,
                        u.value.to_vec(),
                    )
                })
                .collect();
            move |store| {
                let updates: Vec<_> = updates
                    .iter()
                    .map(|(object, head, published, value)| Update {
                        object: *object,
                        expected_head: *head,
                        expected_published: *published,
                        value,
                    })
                    .collect();
                store.commit(&updates)
            }
        })
    }

    pub fn try_edit_components(
        &self,
        object: ObjectKey,
        expected_head: Revision,
        expected_published: Option<Revision>,
        updates: &[ComponentUpdate<'_>],
        options: Options,
    ) -> Result<Pending<Revision>, AdmissionError> {
        if updates.is_empty() || updates.len() > 256 {
            return Err(self.rejection(AdmissionError::InvalidRequest));
        }
        let bytes = updates
            .iter()
            .try_fold(0usize, |n, u| n.checked_add(u.value.map_or(0, |v| v.len())))
            .ok_or_else(|| self.rejection(AdmissionError::TooLarge))?;
        self.submit(bytes, options, || {
            let updates: Vec<_> = updates
                .iter()
                .map(|u| (u.id, u.value.map(<[u8]>::to_vec)))
                .collect();
            move |store| {
                let updates: Vec<_> = updates
                    .iter()
                    .map(|(id, value)| ComponentUpdate {
                        id: *id,
                        value: value.as_deref(),
                    })
                    .collect();
                store.edit_components(object, expected_head, expected_published, &updates)
            }
        })
    }

    pub fn try_publish(
        &self,
        object: ObjectKey,
        revision: Revision,
        expected_published: Revision,
        options: Options,
    ) -> Result<Pending<Revision>, AdmissionError> {
        self.submit(0, options, || {
            move |store| store.publish(object, revision, expected_published)
        })
    }

    pub fn try_get(
        &self,
        object: ObjectKey,
        options: Options,
    ) -> Result<Pending<Snapshot>, AdmissionError> {
        self.submit(0, options, || move |store| store.get(object))
    }

    pub fn try_revision(
        &self,
        object: ObjectKey,
        revision: Revision,
        options: Options,
    ) -> Result<Pending<Snapshot>, AdmissionError> {
        self.submit(0, options, || move |store| store.revision(object, revision))
    }

    pub fn try_get_components(
        &self,
        object: ObjectKey,
        options: Options,
    ) -> Result<Pending<ComponentSnapshot>, AdmissionError> {
        self.submit(0, options, || move |store| store.get_components(object))
    }

    pub fn try_component_revision(
        &self,
        object: ObjectKey,
        revision: Revision,
        options: Options,
    ) -> Result<Pending<ComponentSnapshot>, AdmissionError> {
        self.submit(0, options, || {
            move |store| store.component_revision(object, revision)
        })
    }

    pub fn try_status(
        &self,
        object: ObjectKey,
        options: Options,
    ) -> Result<Pending<ObjectStatus>, AdmissionError> {
        self.submit(0, options, || {
            move |store| {
                let (history_floor, published) = store.history_bounds(object)?;
                Ok(ObjectStatus {
                    head: store.head(object),
                    published,
                    history_floor,
                })
            }
        })
    }

    pub fn try_compact(
        &self,
        retention: Retention,
        options: Options,
    ) -> Result<Pending<Compaction>, AdmissionError> {
        self.submit(0, options, || move |store| store.compact(retention))
    }

    pub fn try_set_limits(
        &self,
        limits: Limits,
        options: Options,
    ) -> Result<Pending<()>, AdmissionError> {
        self.submit(0, options, || move |store| store.set_limits(limits))
    }

    pub fn try_publication_cursor(
        &self,
        object: ObjectKey,
        after: Revision,
        options: Options,
    ) -> Result<Pending<PublicationCursor>, AdmissionError> {
        self.submit(0, options, || {
            move |store| store.publication_cursor(object, after.get())
        })
    }

    pub fn try_publication_after(
        &self,
        cursor: &PublicationCursor,
        options: Options,
    ) -> Result<Pending<Option<Publication>>, AdmissionError> {
        self.submit(0, options, || {
            let cursor = cursor.clone();
            move |store| store.publication_after(&cursor)
        })
    }

    pub fn try_component_publication_after(
        &self,
        cursor: &PublicationCursor,
        options: Options,
    ) -> Result<Pending<Option<ComponentPublication>>, AdmissionError> {
        self.submit(0, options, || {
            let cursor = cursor.clone();
            move |store| store.component_publication_after(&cursor)
        })
    }
}
