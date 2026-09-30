//! Validated publication positions with local Store ownership.
#![forbid(unsafe_code)]

use super::{Error, ObjectKey, Result, Revision, Snapshot, Store};
use std::{
    ops::Bound::{Excluded, Included},
    sync::Arc,
};

/// Immutable position in one object's publication history, issued by one open
/// Store. Cloning preserves the position for retries; reading never advances it.
///
/// This is local validation evidence, not capability authority. It does not keep
/// a file locked or prevent retention from expiring the position. Persist the
/// object and `after()` separately and explicitly resume after reopening a Store.
#[derive(Clone, Debug)]
#[must_use]
pub struct PublicationCursor {
    owner: Arc<()>,
    object: ObjectKey,
    after: Revision,
}
impl PublicationCursor {
    #[must_use]
    pub fn object(&self) -> ObjectKey {
        self.object
    }
    /// Numeric position for a wire or durable checkpoint. Zero is the start of
    /// history; a positive position identifies a publication, never a draft.
    #[must_use]
    pub fn after(&self) -> Revision {
        self.after
    }
}

/// One published snapshot and its matching position for the next read.
#[derive(Clone)]
#[must_use]
pub struct Publication {
    snapshot: Snapshot,
    cursor: PublicationCursor,
}
impl Publication {
    #[must_use]
    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }
    pub fn cursor(&self) -> &PublicationCursor {
        &self.cursor
    }
    pub fn into_parts(self) -> (Snapshot, PublicationCursor) {
        (self.snapshot, self.cursor)
    }
}

impl Store {
    /// Validate an explicit wire/checkpoint position and bind it to this open
    /// Store and object. The caller must authorize access to the object separately.
    /// Zero is accepted only while the entire publication history is retained.
    pub fn publication_cursor(&self, object: ObjectKey, after: u64) -> Result<PublicationCursor> {
        let after = Revision::new(after);
        self.check_publication_position(object, after)?;
        Ok(PublicationCursor {
            owner: self.cursor_owner.clone(),
            object,
            after,
        })
    }

    /// Read the first publication strictly after this cursor. The input remains
    /// unchanged for retry; the returned publication contains the next cursor.
    /// Cursors from another open Store (including a reopen of the same path) are
    /// rejected. Current retention and writer health are rechecked on every read.
    pub fn publication_after(&self, cursor: &PublicationCursor) -> Result<Option<Publication>> {
        if !Arc::ptr_eq(&self.cursor_owner, &cursor.owner) {
            return Err(Error::ForeignCursor);
        }
        self.check_publication_position(cursor.object, cursor.after)?;
        self.publications
            .range((
                Excluded((cursor.object, cursor.after)),
                Included((cursor.object, Revision::MAX)),
            ))
            .next()
            .map(|&(_, revision)| {
                Ok(Publication {
                    snapshot: self.revision(cursor.object, revision)?,
                    cursor: PublicationCursor {
                        owner: cursor.owner.clone(),
                        object: cursor.object,
                        after: revision,
                    },
                })
            })
            .transpose()
    }

    fn check_publication_position(&self, object: ObjectKey, after: Revision) -> Result<()> {
        let (floor, published) = self.history_bounds(object)?;
        if after < floor {
            return Err(Error::HistoryExpired { floor });
        }
        if after > published
            || (after != Revision::INITIAL && !self.publications.contains(&(object, after)))
        {
            return Err(Error::InvalidCursor);
        }
        Ok(())
    }
}
