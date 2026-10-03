//! Application-defined source identity, resolution and bounded input.
use super::{compile_graph, File, Loaded, Loader, Message};
use crate::{Diagnostic, MAX_SOURCE_BYTES};
use std::{
    collections::BTreeMap,
    io::{self, Read},
};

/// A resolved schema or embedded file in an application-defined namespace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceFile {
    /// Opaque cache key. Aliases of the same file must return the same key.
    /// Distinct files (including files in different repositories) must not share
    /// a key. Identity includes the context needed to resolve that file's imports.
    pub identity: String,
    /// Logical filename for diagnostics, schema display names and requested-file
    /// output. The first spelling loaded for an identity wins. A caller passing
    /// the request to a generator must choose suitable output filenames.
    pub filename: String,
}

/// Custom source resolution and I/O for [`SourceCompiler`].
///
/// The provider controls the complete namespace: the compiler never falls back
/// to disk, environment variables or the in-memory parser's import roots.
/// It does not normalize import strings. Providers implement their own relative
/// paths, absolute import roots, alias identity and access policy.
///
/// Identities must stay stable within a compilation/session and denote the same
/// bytes and import context. Successfully compiled schema/embed inputs are cached
/// separately by identity. A new compilation starts fresh. Both returned strings
/// must be nonempty, at most 4096 bytes and contain no control characters.
///
/// Callbacks run synchronously. Neither `Send` nor `Sync` is required. Compiler
/// state is transactional, but callback side effects (such as logs or I/O) cannot
/// be rolled back. Providers should allow retries after failure.
pub trait SourceProvider {
    /// Resolve a requested name (`from == None`) or an import/embed from a
    /// previously resolved identity. `path` is the exact requested name or decoded
    /// source string. Return `None` only for absence; other failures are errors.
    fn resolve(&self, from: Option<&str>, path: &str) -> io::Result<Option<SourceFile>>;

    /// Open a resolved identity. Called only when this schema/embed identity is
    /// not cached. The compiler reads at most 4 MiB plus one byte (to detect an
    /// oversized input); its total 16 MiB input budget still applies. A provider
    /// may borrow its storage through the returned reader. Read failures become
    /// diagnostics at the importing/embedding expression or requested filename.
    fn open(&self, identity: &str) -> io::Result<Box<dyn Read + '_>>;
}

/// Compile schemas from custom source callbacks, without implicit filesystem I/O.
///
/// This uses the same grammar, lazy dependency selection, graph/work limits and
/// all-or-error compilation as [`super::SchemaParser`]. [`Self::parse_schemas`]
/// produces an owned runtime snapshot; [`Self::parse_session`] supports lazy
/// declaration loading while borrowing the provider. Callbacks do not imply
/// filesystem dependencies, so snapshots' `dependencies()` lists are empty.
///
/// ```
/// use capntproto_compiler::{SourceCompiler, SourceFile, SourceProvider};
/// use std::io::{self, Read};
/// struct Config;
/// impl SourceProvider for Config {
///     fn resolve(&self, from: Option<&str>, path: &str) -> io::Result<Option<SourceFile>> {
///         Ok((from.is_none() && path == "config").then(|| SourceFile {
///             identity: "config:v1".into(), filename: "config.capnp".into(),
///         }))
///     }
///     fn open(&self, identity: &str) -> io::Result<Box<dyn Read + '_>> {
///         assert_eq!(identity, "config:v1");
///         Ok(Box::new(&b"@0xaaaaaaaaaaaaaaaa; struct Config { port @0 :UInt16 = 80; }"[..]))
///     }
/// }
/// let provider = Config;
/// let parsed = SourceCompiler::new(&provider).parse_schemas(&["config"])?;
/// assert!(parsed.get_file("config.capnp")?.get_nested("Config").is_ok());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// A session cannot outlive its borrowed provider:
/// ```compile_fail
/// use capntproto_compiler::{SchemaSession, SourceCompiler, SourceProvider};
/// fn escape(provider: &dyn SourceProvider) -> SchemaSession<'static> {
///     SourceCompiler::new(provider).parse_session(&["entry"]).unwrap()
/// }
/// ```
pub struct SourceCompiler<'a> {
    pub(crate) provider: &'a dyn SourceProvider,
    pub(crate) allow_missing_ids: bool,
}

impl<'a> SourceCompiler<'a> {
    pub fn new(provider: &'a dyn SourceProvider) -> Self {
        Self {
            provider,
            allow_missing_ids: false,
        }
    }

    /// Choose the same file-ID policy as [`super::SchemaParser::set_file_ids_required`].
    /// Defaults to requiring explicit IDs. Sessions capture this setting;
    /// providers still control file identity while generated IDs use OS randomness.
    pub fn set_file_ids_required(&mut self, required: bool) -> &mut Self {
        self.allow_missing_ids = !required;
        self
    }

    /// Compile requested provider names into a standard CodeGeneratorRequest.
    /// Repeated identities are deduplicated in request order. No cache is retained
    /// between calls; use a session for explicit, transactional lazy loading.
    pub fn compile(&self, requested: &[&str]) -> Result<Message, Diagnostic> {
        compile_graph(&Input(self.provider), requested, self.allow_missing_ids)
    }
}

#[derive(Clone, Copy)]
pub(super) struct Input<'a>(pub &'a dyn SourceProvider);

impl Loader for Input<'_> {
    fn load(
        &self,
        from: Option<&File>,
        path: &str,
        known: &BTreeMap<String, usize>,
    ) -> Result<Loaded, String> {
        let resolved = self
            .0
            .resolve(from.map(|file| file.identity.as_str()), path)
            .map_err(|e| format!("resolving {path}: {e}"))?
            .ok_or_else(|| format!("schema file not found: {path}"))?;
        for (label, value) in [
            ("identity", &resolved.identity),
            ("filename", &resolved.filename),
        ] {
            if value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
                return Err(format!("invalid provider {label}"));
            }
        }
        let mut bytes = Vec::new();
        if !known.contains_key(&resolved.identity) {
            self.0
                .open(&resolved.identity)
                .and_then(|reader| {
                    reader
                        .take(MAX_SOURCE_BYTES as u64 + 1)
                        .read_to_end(&mut bytes)
                })
                .map_err(|e| format!("reading {}: {e}", resolved.filename))?;
        }
        Ok(Loaded {
            // Path interpretation belongs entirely to the provider.
            rooted: false,
            identity: resolved.identity,
            filename: resolved.filename,
            bytes,
        })
    }
}
