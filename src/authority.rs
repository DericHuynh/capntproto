//! Holder-bound grants. Release of a reference does not revoke a grant.
#![forbid(unsafe_code)]
pub use crate::object_ids::{ObjectGeneration, ObjectId};
use std::rc::Rc;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rights(u8);
impl Rights {
    pub const GET: Self = Self(1);
    pub const PUT: Self = Self(2);
    pub const PUBLISH: Self = Self(4);
    pub const SUBSCRIBE: Self = Self(8);
    pub const DELEGATE: Self = Self(16);
    pub const ALL: Self = Self(31);
    pub const VIEW: Self = Self(9);
    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
    pub fn bits(self) -> u8 {
        self.0
    }
    pub fn from_bits(bits: u8) -> Option<Self> {
        (bits & !Self::ALL.0 == 0).then_some(Self(bits))
    }
}
/// Immutable object, generation, holder and attenuated rights with shared
/// revocation lineage. Cloning preserves every binding; changing the holder
/// requires [`Self::delegate`], which checks delegation authority and rights.
/// The owning service remains responsible for trusted root issuance and for
/// binding holder metadata to authenticated peers.
///
/// ```compile_fail,E0616
/// fn retarget(grant: &mut capntproto::authority::Grant) {
///     grant.object = 99;
/// }
/// ```
/// ```compile_fail,E0616
/// fn replace_generation(grant: &mut capntproto::authority::Grant) {
///     grant.generation = 99;
/// }
/// ```
/// ```compile_fail,E0616
/// fn replace_holder(grant: &mut capntproto::authority::Grant) {
///     grant.holder = [9; 32];
/// }
/// ```
#[must_use = "a grant must be retained or installed to carry authority"]
#[derive(Clone)]
pub struct Grant {
    object: ObjectId,
    generation: ObjectGeneration,
    holder: [u8; 32],
    rights: Rights,
    lineage: Vec<Rc<tokio::sync::watch::Sender<bool>>>,
}
impl Grant {
    /// Called only at the owning service's trusted bootstrap boundary.
    pub fn root(
        object: ObjectId,
        generation: ObjectGeneration,
        holder: [u8; 32],
        rights: Rights,
    ) -> Self {
        Self {
            object,
            generation,
            holder,
            rights,
            lineage: vec![Rc::new(tokio::sync::watch::channel(true).0)],
        }
    }
    #[must_use]
    pub fn object(&self) -> ObjectId {
        self.object
    }
    #[must_use]
    pub fn generation(&self) -> ObjectGeneration {
        self.generation
    }
    #[must_use]
    pub fn holder(&self) -> [u8; 32] {
        self.holder
    }
    pub fn is_live(&self) -> bool {
        self.lineage.iter().all(|x| *x.borrow())
    }
    pub fn allows(&self, right: Rights) -> bool {
        self.rights.contains(right) && self.is_live()
    }
    pub fn rights(&self) -> Rights {
        self.rights
    }
    pub fn revoke(&self) {
        self.lineage.last().unwrap().send_replace(false);
    }
    /// Wait for revocation of this grant or any ancestor, including a
    /// revocation that happened before the wait was first polled.
    pub async fn when_revoked(&self) {
        let waits = self.lineage.iter().map(|live| {
            let mut changed = live.subscribe();
            Box::pin(async move {
                loop {
                    if !*changed.borrow_and_update() || changed.changed().await.is_err() {
                        return;
                    }
                }
            })
        });
        futures::future::select_all(waits).await;
    }
    pub fn delegate(&self, holder: [u8; 32], rights: Rights) -> Option<Self> {
        if !self.allows(Rights::DELEGATE) || !self.rights.contains(rights) {
            return None;
        }
        let mut lineage = self.lineage.clone();
        lineage.push(Rc::new(tokio::sync::watch::channel(true).0));
        Some(Self {
            object: self.object,
            generation: self.generation,
            holder,
            rights,
            lineage,
        })
    }
}
