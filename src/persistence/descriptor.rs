//! Validated, immutable descriptions of persistent authority. These values are
//! metadata; constructing or decoding one does not grant a capability.
#![forbid(unsafe_code)]

use super::ObjectKind;
use crate::authority::{ObjectGeneration, ObjectId, Rights};
use serde::{Deserialize, Serialize};

/// Realm-local description of a restorable facet. Its bindings are immutable;
/// cloning preserves them. The registered factory must enforce the generation
/// and exact rights, and the trusted host must bind the intended capability.
///
/// Serialization preserves the numeric ledger format. Deserialization validates
/// each ID, rights, and the exact field set before constructing this value.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[must_use = "a descriptor describes authority; retain it or bind it to the intended capability"]
pub struct Descriptor {
    kind: ObjectKind,
    object: ObjectId,
    generation: ObjectGeneration,
    #[serde(serialize_with = "serialize_rights")]
    rights: Rights,
}

fn serialize_rights<S: serde::Serializer>(
    rights: &Rights,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_u8(rights.bits())
}

impl<'de> Deserialize<'de> for Descriptor {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            kind: ObjectKind,
            object: ObjectId,
            generation: ObjectGeneration,
            rights: u8,
        }
        let fields = Fields::deserialize(deserializer)?;
        let rights = Rights::from_bits(fields.rights)
            .ok_or_else(|| serde::de::Error::custom("invalid descriptor rights"))?;
        Ok(Self::new(
            fields.kind,
            fields.object,
            fields.generation,
            rights,
        ))
    }
}

impl Descriptor {
    /// Infallible once each identifier and the rights value have been checked.
    pub const fn new(
        kind: ObjectKind,
        object: ObjectId,
        generation: ObjectGeneration,
        rights: Rights,
    ) -> Self {
        Self {
            kind,
            object,
            generation,
            rights,
        }
    }
    #[must_use]
    pub const fn kind(&self) -> ObjectKind {
        self.kind
    }
    #[must_use]
    pub const fn object(&self) -> ObjectId {
        self.object
    }
    #[must_use]
    pub const fn generation(&self) -> ObjectGeneration {
        self.generation
    }
    #[must_use]
    pub const fn rights(&self) -> Rights {
        self.rights
    }
}
