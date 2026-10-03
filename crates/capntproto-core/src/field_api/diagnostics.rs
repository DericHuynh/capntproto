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
    pub fn error(self, mut error: Error) -> Error {
        use alloc::format;
        let prefix = match self {
            Self::Unknown => return error,
            Self::Field {
                schema,
                name,
                ordinal: Some(n),
            } => format!("{schema}.{name} (@{n})"),
            Self::Field {
                schema,
                name,
                ordinal: None,
            } => format!("{schema}.{name}"),
            Self::Index(n) => format!("[{n}]"),
            Self::Union(schema) => format!("{schema} union"),
        };
        error.extra = if error.extra.is_empty() {
            prefix
        } else {
            format!("{prefix}: {}", error.extra)
        };
        error
    }

    #[doc(hidden)]
    pub fn run<T>(self, operation: impl FnOnce() -> Result<T>) -> Result<T> {
        operation().map_err(|error| self.error(error))
    }
}

#[doc(hidden)]
pub fn conversion(mut error: Error, schema: &str, direction: &str) -> Error {
    error.extra = alloc::format!("native {direction} {schema}: {}", error.extra);
    error
}
