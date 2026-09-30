//! Immutable reflection snapshots produced by the textual frontend.
use crate::{Diagnostic, FileCompiler, SchemaParser, SourceCompiler};
use capnp::{
    message::{Builder, HeapAllocator},
    schema_capnp::{code_generator_request, node},
    schema_loader::{Limits, Schema, SchemaLoader},
};
use std::{
    collections::BTreeMap,
    fmt,
    path::{Path, PathBuf},
};

/// A textual compilation diagnostic or a runtime schema validation failure.
#[derive(Debug)]
pub enum ParseError {
    Compile(Diagnostic),
    Load(capnp::Error),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Compile(error) => error.fmt(f),
            Self::Load(error) => write!(f, "runtime schema validation: {error}"),
        }
    }
}

impl std::error::Error for ParseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::Compile(error) => error,
            Self::Load(error) => error,
        })
    }
}

impl From<Diagnostic> for ParseError {
    fn from(error: Diagnostic) -> Self {
        Self::Compile(error)
    }
}

impl From<capnp::Error> for ParseError {
    fn from(error: capnp::Error) -> Self {
        Self::Load(error)
    }
}

impl SchemaParser {
    /// Compile and validate a fresh, owned runtime reflection snapshot.
    /// The snapshot is independent of this parser and its registered sources.
    pub fn parse_schemas(&self, requested: &[&str]) -> Result<ParsedSchemas, ParseError> {
        self.parse_schemas_with_limits(requested, Limits::default())
    }

    /// Like [`Self::parse_schemas`], with explicit runtime loader limits in
    /// addition to the frontend's compilation limits. No partial result escapes.
    pub fn parse_schemas_with_limits(
        &self,
        requested: &[&str],
        limits: Limits,
    ) -> Result<ParsedSchemas, ParseError> {
        ParsedSchemas::load(self.parse(requested)?, Vec::new(), limits)
    }
}

impl FileCompiler {
    /// Compile filesystem inputs into an owned runtime reflection snapshot.
    /// Loaded input paths remain available through [`ParsedSchemas::dependencies`].
    /// Import and filesystem boundaries are identical to [`Self::compile`].
    pub fn parse_schemas(
        &self,
        requested: &[impl AsRef<Path>],
    ) -> Result<ParsedSchemas, ParseError> {
        self.parse_schemas_with_limits(requested, Limits::default())
    }

    /// Like [`Self::parse_schemas`], with explicit runtime loader limits.
    pub fn parse_schemas_with_limits(
        &self,
        requested: &[impl AsRef<Path>],
        limits: Limits,
    ) -> Result<ParsedSchemas, ParseError> {
        let compilation = self.compile_with_dependencies(requested)?;
        ParsedSchemas::load(compilation.message, compilation.dependencies, limits)
    }
}

impl SourceCompiler<'_> {
    /// Compile provider inputs into a validated, owned runtime snapshot.
    /// The provider need not outlive the returned snapshot.
    pub fn parse_schemas(&self, requested: &[&str]) -> Result<ParsedSchemas, ParseError> {
        self.parse_schemas_with_limits(requested, Limits::default())
    }

    /// Like [`Self::parse_schemas`], with explicit runtime loader limits.
    pub fn parse_schemas_with_limits(
        &self,
        requested: &[&str],
        limits: Limits,
    ) -> Result<ParsedSchemas, ParseError> {
        ParsedSchemas::load(self.compile(requested)?, Vec::new(), limits)
    }
}

/// An immutable set of parsed schemas, their compiler metadata and input paths.
///
/// Requested files and their declarations are compiled eagerly; imports retain
/// the frontend's dependency-only selection. Looking up a declaration does not
/// compile more source. Request an imported file explicitly to include all its
/// declarations. Aliases are resolved during compilation but are not declarations
/// in the emitted schema, so they are not exposed by nested-name lookup.
/// Use [`crate::SchemaSession`] for alias lookup and lazy declaration compilation.
///
/// Runtime schema validation happens before this object is returned. Handles
/// borrow this snapshot, while the source parser/files can be dropped or changed.
/// The underlying loader is exposed read-only so schemas cannot be replaced under
/// retained source metadata. The snapshot is not a concurrent C++ parser cache.
///
/// ```
/// use capnp::schema_loader::dynamic::{Builder, Value};
/// let mut parser = capnp_compiler::SchemaParser::new();
/// parser.add_source("config.capnp", "@0xaaaaaaaaaaaaaaaa; struct Config { port @0 :UInt16 = 80; }")?;
/// let parsed = parser.parse_schemas(&["config.capnp"])?;
/// drop(parser);
/// let config = parsed.get_file("config.capnp")?.get_nested("Config")?;
/// let mut message = capnp::message::Builder::new_default();
/// let mut value = Builder::init(message.init_root(), config.schema())?;
/// assert!(matches!(value.as_reader().get_named("port")?, Value::UInt16(80)));
/// value.set_named("port", Value::UInt16(8080))?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub struct ParsedSchemas {
    message: Builder<HeapAllocator>,
    loader: SchemaLoader,
    source_info: BTreeMap<u64, u32>,
    requested: Vec<(String, u64)>,
    dependencies: Vec<PathBuf>,
}

impl ParsedSchemas {
    pub(crate) fn serialized_request(&self) -> Vec<u8> {
        capnp::serialize::write_message_to_words(&self.message)
    }
    // Accept only frontend-produced requests. This is not an untrusted binary
    // request loader; the runtime SchemaLoader supplies that separate API.
    pub(crate) fn load(
        message: Builder<HeapAllocator>,
        dependencies: Vec<PathBuf>,
        limits: Limits,
    ) -> Result<Self, ParseError> {
        let request = message.get_root_as_reader::<code_generator_request::Reader<'_>>()?;
        let mut loader = SchemaLoader::new(limits);
        loader.load_request(request)?;
        let source_info = request
            .get_source_info()?
            .iter()
            .enumerate()
            .map(|(index, info)| (info.get_id(), index as u32))
            .collect();
        let requested = request
            .get_requested_files()?
            .iter()
            .map(|file| {
                loader.get(file.get_id())?;
                Ok((file.get_filename()?.to_str()?.to_owned(), file.get_id()))
            })
            .collect::<capnp::Result<_>>()?;
        Ok(Self {
            message,
            loader,
            source_info,
            requested,
            dependencies,
        })
    }

    /// The unchanged compiler request, including per-file identifier references.
    pub fn request(&self) -> code_generator_request::Reader<'_> {
        self.message
            .get_root_as_reader()
            .expect("validated frontend request")
    }

    /// The runtime loader. Clone it if a separate mutable/native-registered
    /// loader is needed; mutations to that clone cannot change this snapshot.
    pub fn loader(&self) -> &SchemaLoader {
        &self.loader
    }

    /// Loaded filesystem inputs, or an empty slice for an in-memory parser.
    /// See [`crate::FileCompilation::dependencies`] for rebuild-tracking details.
    pub fn dependencies(&self) -> &[PathBuf] {
        &self.dependencies
    }

    /// Requested files in first-request order, after identity deduplication.
    pub fn requested_files(&self) -> impl ExactSizeIterator<Item = (&str, ParsedSchema<'_>)> {
        self.requested.iter().map(|(name, id)| {
            (
                name.as_str(),
                self.get(*id).expect("validated requested file"),
            )
        })
    }

    /// Look up a requested file using its exact logical filename in the request.
    /// This does not interpret filesystem paths, import spellings or aliases.
    pub fn get_file(&self, filename: &str) -> capnp::Result<ParsedSchema<'_>> {
        let id = self
            .requested
            .iter()
            .find(|(name, _)| name == filename)
            .map(|(_, id)| *id)
            .ok_or_else(|| capnp::Error::failed(format!("file was not requested: {filename}")))?;
        self.get(id)
    }

    /// Get a loaded declaration, including implicit group and method structures.
    /// Missing dependency stubs are not returned as parsed declarations.
    pub fn get(&self, id: u64) -> capnp::Result<ParsedSchema<'_>> {
        let schema = self.loader.get(id)?;
        if schema.is_stub() {
            return Err(capnp::Error::failed(format!(
                "schema {id:#x} was not compiled"
            )));
        }
        Ok(ParsedSchema {
            owner: self,
            schema,
        })
    }

    /// All compiled declarations, including imported dependencies and synthetic
    /// group/method nodes, in schema-ID order. Excludes dependency stubs.
    pub fn get_all_loaded(&self) -> impl Iterator<Item = ParsedSchema<'_>> {
        self.loader.get_all_loaded().map(|schema| ParsedSchema {
            owner: self,
            schema,
        })
    }

    /// Documentation and member ranges by ID, including implicit group/method
    /// structures. Unknown IDs return `None`.
    pub fn source_info(&self, id: u64) -> Option<node::source_info::Reader<'_>> {
        self.source_info.get(&id).map(|&index| {
            self.request()
                .get_source_info()
                .expect("validated source info")
                .get(index)
        })
    }
}

/// A declaration and its source metadata, borrowing a [`ParsedSchemas`] owner.
///
/// Handles cannot outlive the snapshot:
/// ```compile_fail
/// let escaped = {
///     let mut parser = capnp_compiler::SchemaParser::new();
///     parser.add_source("s.capnp", "@0xaaaaaaaaaaaaaaaa; struct S {}").unwrap();
///     let parsed = parser.parse_schemas(&["s.capnp"]).unwrap();
///     parsed.get_file("s.capnp").unwrap().get_nested("S").unwrap()
/// };
/// let _ = escaped.schema();
/// ```
#[derive(Clone)]
pub struct ParsedSchema<'a> {
    owner: &'a ParsedSchemas,
    schema: Schema<'a>,
}

impl<'a> ParsedSchema<'a> {
    /// Runtime reflection handle for field lookup, brands and dynamic messages.
    pub fn schema(&self) -> Schema<'a> {
        self.schema.clone()
    }

    /// Look up a direct declaration by its schema name (case-sensitive).
    /// This does not resolve aliases, dotted paths, fields, groups or methods.
    /// A declared but uncompiled imported child returns an error, not `None`;
    /// request that imported file explicitly to compile its full subtree.
    pub fn find_nested(&self, name: &str) -> capnp::Result<Option<Self>> {
        for nested in self.schema.get_proto().get_nested_nodes()? {
            if nested.get_name()? == name {
                return self.owner.get(nested.get_id()).map(Some);
            }
        }
        Ok(None)
    }

    /// Like [`Self::find_nested`], returning an error for an absent declaration.
    pub fn get_nested(&self, name: &str) -> capnp::Result<Self> {
        self.find_nested(name)?
            .ok_or_else(|| capnp::Error::failed(format!("no such nested declaration: {name}")))
    }

    /// Direct declarations in schema order, excluding aliases and implicit nodes.
    /// Returns an error if any listed child was not compiled (see [`Self::find_nested`]).
    pub fn get_all_nested(&self) -> capnp::Result<Vec<Self>> {
        self.schema
            .get_proto()
            .get_nested_nodes()?
            .iter()
            .map(|child| self.owner.get(child.get_id()))
            .collect()
    }

    /// Source comments and ranges for this declaration.
    pub fn source_info(&self) -> Option<node::source_info::Reader<'a>> {
        self.owner.source_info(self.schema.id())
    }
}
