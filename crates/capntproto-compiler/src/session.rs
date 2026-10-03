//! Explicit, transactional loading of additional textual declarations.
use crate::{
    source, FileCompiler, ParseError, ParsedSchema, ParsedSchemas, SchemaParser, SourceCompiler,
};
use capnp::schema_loader::Limits;
use std::{collections::BTreeMap, path::Path};

impl SchemaParser {
    /// Start a session that can compile additional declarations on demand.
    /// The session borrows this parser's immutable virtual filesystem.
    pub fn parse_session(&self, requested: &[&str]) -> Result<SchemaSession<'_>, ParseError> {
        self.parse_session_with_limits(requested, Limits::default())
    }

    /// Like [`Self::parse_session`], with explicit runtime loader limits.
    pub fn parse_session_with_limits(
        &self,
        requested: &[&str],
        limits: Limits,
    ) -> Result<SchemaSession<'_>, ParseError> {
        SchemaSession::new(source::Session::memory(self, requested)?, limits)
    }
}

impl FileCompiler {
    /// Start a filesystem session. Search paths are captured as absolute paths;
    /// the compiler configuration need not remain alive. Successfully read
    /// files are cached; previously unused imports/embeds are opened on demand.
    pub fn parse_session(
        &self,
        requested: &[impl AsRef<Path>],
    ) -> Result<SchemaSession<'static>, ParseError> {
        self.parse_session_with_limits(requested, Limits::default())
    }

    /// Like [`Self::parse_session`], with explicit runtime loader limits.
    pub fn parse_session_with_limits(
        &self,
        requested: &[impl AsRef<Path>],
        limits: Limits,
    ) -> Result<SchemaSession<'static>, ParseError> {
        SchemaSession::new(source::Session::disk(self, requested)?, limits)
    }
}

impl<'a> SourceCompiler<'a> {
    /// Start a transactional lazy session borrowing the source provider.
    /// The compiler configuration itself need not remain alive. Successful
    /// inputs stay cached; callback side effects are not rolled back on failure.
    pub fn parse_session(&self, requested: &[&str]) -> Result<SchemaSession<'a>, ParseError> {
        self.parse_session_with_limits(requested, Limits::default())
    }

    /// Like [`Self::parse_session`], with explicit runtime loader limits.
    pub fn parse_session_with_limits(
        &self,
        requested: &[&str],
        limits: Limits,
    ) -> Result<SchemaSession<'a>, ParseError> {
        SchemaSession::new(source::Session::custom(self, requested)?, limits)
    }
}

/// A bounded schema parser session with explicit lazy declaration loading.
///
/// Read through [`Self::schemas`] to borrow a validated snapshot. Loading needs
/// an exclusive borrow, so existing schema handles and dynamic views cannot be
/// invalidated. Each extension stages source discovery, compilation and runtime
/// validation before committing; failures preserve the previous snapshot,
/// cached inputs and dependency list. Requested-file order stays unchanged.
///
/// Declarations can be loaded by ID or nested name, including declaration
/// aliases. Like pinned C++'s `ParsedSchema::findNested()`, alias lookup returns
/// the underlying declaration without applying its generic bindings. Fields and
/// methods are not nested declarations. Loading includes dependencies, not unused children
/// or siblings. It does not add the containing file to the requested-file list.
///
/// Successfully loaded files remain cached even if modified or deleted later.
/// Start a new session to observe changes to these files. Each extension uses
/// the frontend's work/expansion limits across the complete accumulated selection;
/// graph/input and loader limits apply to the whole session. Sessions are not
/// concurrent caches and do not mutate through shared handles. Use
/// [`crate::ConcurrentSchemaParser`] for worker-owned sessions shared by threads.
///
/// ```
/// let mut parser = capntproto_compiler::SchemaParser::new();
/// parser.add_source("main.capnp", "@0xaaaaaaaaaaaaaaaa; using I = import \"types.capnp\"; struct Root { x @0 :I.Item; }")?;
/// parser.add_source("types.capnp", "@0xbbbbbbbbbbbbbbbb; struct Item {} struct Extra { x @0 :Text; }")?;
/// let mut session = parser.parse_session(&["main.capnp"])?;
/// let extra = session.get_nested(0xbbbbbbbbbbbbbbbb, "Extra")?;
/// assert_eq!(extra.schema().field("x")?.get_proto().get_name()?, "x");
/// let snapshot = session.into_schemas();
/// drop(parser);
/// assert!(snapshot.get(0xbbbbbbbbbbbbbbbb)?.get_nested("Extra").is_ok());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// A live schema handle prevents an extension:
/// ```compile_fail
/// let mut parser = capntproto_compiler::SchemaParser::new();
/// parser.add_source("main.capnp", "@0xaaaaaaaaaaaaaaaa; struct S {}").unwrap();
/// let mut session = parser.parse_session(&["main.capnp"]).unwrap();
/// let schema = session.schemas().get_file("main.capnp").unwrap();
/// session.load(0xaaaaaaaaaaaaaaaa).unwrap();
/// let _ = schema.schema();
/// ```
pub struct SchemaSession<'a> {
    source: source::Session<'a>,
    schemas: ParsedSchemas,
    limits: Limits,
    // Only actual aliases are cached, bounding entries by the source graph's
    // alias limit. Missing arbitrary lookup names must not grow this table.
    aliases: BTreeMap<(u64, String), u64>,
    revision: u64,
}

impl<'a> SchemaSession<'a> {
    fn new(mut source: source::Session<'a>, limits: Limits) -> Result<Self, ParseError> {
        let compilation = source.compile()?;
        let schemas = ParsedSchemas::load(compilation.message, compilation.dependencies, limits)?;
        Ok(Self {
            source,
            schemas,
            limits,
            aliases: BTreeMap::new(),
            revision: 0,
        })
    }

    /// Borrow the current validated schemas, source metadata and dependencies.
    pub fn schemas(&self) -> &ParsedSchemas {
        &self.schemas
    }

    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    /// Finish loading and detach the owned snapshot from its source provider.
    pub fn into_schemas(self) -> ParsedSchemas {
        self.schemas
    }

    fn extend(&mut self, ids: &[u64]) -> Result<(), ParseError> {
        if ids.iter().all(|&id| self.schemas.get(id).is_ok()) {
            return Ok(());
        }
        let mut staged = self.source.clone();
        for &id in ids {
            staged.require(id)?;
        }
        self.commit(staged)
    }

    fn commit(&mut self, mut staged: source::Session<'a>) -> Result<(), ParseError> {
        let compilation = staged.compile()?;
        let schemas =
            ParsedSchemas::load(compilation.message, compilation.dependencies, self.limits)?;
        self.source = staged;
        self.schemas = schemas;
        self.revision += 1;
        Ok(())
    }

    /// Compile a known source declaration and its dependency closure on demand.
    /// Unknown IDs return a compilation diagnostic. Already loaded IDs do no work.
    pub fn load(&mut self, id: u64) -> Result<ParsedSchema<'_>, ParseError> {
        self.extend(&[id])?;
        Ok(self.schemas.get(id)?)
    }

    /// Find and load a declaration or alias by case-sensitive name. The parent ID
    /// must come from source already discovered by this session. Missing names
    /// return `None` without opening files. Aliases can discover imports and fail;
    /// they return the unbranded declaration, `None` for generic parameters, and
    /// an error for built-ins/list expressions that have no loadable schema node.
    pub fn find_nested(
        &mut self,
        parent: u64,
        name: &str,
    ) -> Result<Option<ParsedSchema<'_>>, ParseError> {
        let id = self
            .source
            .children(parent)?
            .into_iter()
            .find(|(child, _)| child == name)
            .map(|(_, id)| id);
        if let Some(id) = id {
            return self.load(id).map(Some);
        }
        if !self.source.has_alias(parent, name)? {
            return Ok(None);
        }
        let key = (parent, name.to_owned());
        if let Some(id) = self.aliases.get(&key) {
            return Ok(Some(self.schemas.get(*id)?));
        }
        let mut staged = self.source.clone();
        let Some(id) = staged.lookup(parent, name)? else {
            // Do not cache None: resolving this alias may have discovered
            // provisional files that this unsuccessful lookup won't commit.
            return Ok(None);
        };
        staged.require(id)?;
        self.commit(staged)?;
        self.aliases.insert(key, id);
        Ok(Some(self.schemas.get(id)?))
    }

    /// Like [`Self::find_nested`], returning an error when the name is absent.
    pub fn get_nested(&mut self, parent: u64, name: &str) -> Result<ParsedSchema<'_>, ParseError> {
        self.find_nested(parent, name)?.ok_or_else(|| {
            capnp::Error::failed(format!("no such nested declaration: {name}")).into()
        })
    }

    /// Load all direct declarations atomically, returning them in schema order.
    /// Aliases and implicit group/method nodes are not enumerated. Failure to
    /// compile any child leaves the entire session unchanged.
    pub fn get_all_nested(&mut self, parent: u64) -> Result<Vec<ParsedSchema<'_>>, ParseError> {
        let ids: Vec<_> = self
            .source
            .children(parent)?
            .into_iter()
            .map(|(_, id)| id)
            .collect();
        self.extend(&ids)?;
        ids.into_iter()
            .map(|id| self.schemas.get(id).map_err(Into::into))
            .collect()
    }
}
