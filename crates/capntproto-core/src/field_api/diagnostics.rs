//! Lazy diagnostic context for generated field operations. Context is added only
//! on error; it does not traverse a payload or change the error kind or metadata.
use crate::{Error, Result};

/// A schema field or list element. Names come from the schema, before Rust
/// renaming. Groups have no explicit ordinal.
#[derive(Clone, Copy, Debug, Default)]
pub enum Location {
    #[default]
    Unknown,
    Field {
        schema: &'static str,
        name: &'static str,
        ordinal: Option<u16>,
    },
    Index(usize),
    Union(&'static str),
}
impl Location {
    /// Attach this location to an existing failure without replacing it.
    /// Without `alloc`, errors retain their kind but cannot own this context.
    /// Format `Location` separately into caller-supplied storage when needed.
    #[cfg(feature = "alloc")]
    pub fn error(self, mut error: Error) -> Error {
        use alloc::format;
        if matches!(self, Self::Unknown) {
            return error;
        }
        error.extra = if error.extra.is_empty() {
            format!("{self}")
        } else {
            format!("{self}: {}", error.extra)
        };
        error
    }

    /// Preserve the structured error kind without allocating context.
    #[cfg(not(feature = "alloc"))]
    pub fn error(self, error: Error) -> Error {
        error
    }

    #[doc(hidden)]
    pub fn run<T>(self, operation: impl FnOnce() -> Result<T>) -> Result<T> {
        operation().map_err(|error| self.error(error))
    }
}

impl core::fmt::Display for Location {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Unknown => Ok(()),
            Self::Field {
                schema,
                name,
                ordinal: Some(n),
            } => write!(f, "{schema}.{name} (@{n})"),
            Self::Field {
                schema,
                name,
                ordinal: None,
            } => write!(f, "{schema}.{name}"),
            Self::Index(n) => write!(f, "[{n}]"),
            Self::Union(schema) => write!(f, "{schema} union"),
        }
    }
}

#[doc(hidden)]
#[cfg(feature = "alloc")]
pub fn conversion(mut error: Error, schema: &str, direction: &str) -> Error {
    error.extra = alloc::format!("native {direction} {schema}: {}", error.extra);
    error
}

#[cfg(all(test, feature = "alloc"))]
mod tests {
    use super::*;

    #[test]
    fn locations_format_consistently_and_preserve_the_underlying_error() {
        for (location, expected) in [
            (Location::Unknown, ""),
            (
                Location::Field {
                    schema: "Record",
                    name: "label",
                    ordinal: Some(1),
                },
                "Record.label (@1)",
            ),
            (
                Location::Field {
                    schema: "Record",
                    name: "details",
                    ordinal: None,
                },
                "Record.details",
            ),
            (Location::Index(3), "[3]"),
            (Location::Union("Record"), "Record union"),
        ] {
            assert_eq!(alloc::format!("{location}"), expected);
            let plain = location.error(Error::from_kind(crate::ErrorKind::NotPresent));
            assert_eq!(plain.kind, crate::ErrorKind::NotPresent);
            assert_eq!(plain.extra, expected);
            let detailed = location.error(Error::failed("original cause".into()));
            assert_eq!(detailed.kind, crate::ErrorKind::Failed);
            assert_eq!(
                detailed.extra,
                if expected.is_empty() {
                    alloc::string::String::from("original cause")
                } else {
                    alloc::format!("{expected}: original cause")
                }
            );
        }
    }
}
