//! Source ownership, import identity and bounded graph construction.
use crate::syntax::{self, Node, Span};
use crate::{Diagnostic, Source, MAX_SOURCE_BYTES};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod custom;
pub use custom::{SourceCompiler, SourceFile, SourceProvider};

type Message = capnp::message::Builder<capnp::message::HeapAllocator>;
const MAX_FILES: usize = 256;
const MAX_TOTAL_BYTES: usize = 16 * 1024 * 1024;

fn diagnostic(filename: &str, message: impl Into<String>) -> Diagnostic {
    Source { filename, text: "" }.error(Span::default(), message)
}

// Schema paths use '/', independently of the host platform. Absolute imports
// are interpreted by the search path, never as absolute host filesystem paths.
fn normalize(path: &str, allow_parent: bool) -> Result<String, String> {
    if path.len() > 4096
        || path
            .chars()
            .any(|c| c.is_control() || c == '\\' || c == ':')
    {
        return Err("invalid schema path".into());
    }
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => (),
            ".." => match parts.last() {
                Some(&previous) if previous != ".." => {
                    parts.pop();
                }
                _ if allow_parent => parts.push(".."),
                _ => return Err("schema path escapes its root".into()),
            },
            _ => parts.push(part),
        }
    }
    Ok(parts.join("/"))
}

fn relative_to(file: &str, path: &str, allow_parent: bool) -> Result<String, String> {
    let parent = file.rsplit_once('/').map_or("", |(parent, _)| parent);
    normalize(&format!("{parent}/{path}"), allow_parent)
}

/// An in-memory schema parser with a virtual, slash-separated filesystem.
/// Source content comes only from registered files. Add source/embedded files and
/// optional import roots, then compile one or more requested files. Opting into
/// automatic file IDs additionally uses OS randomness.
///
/// Relative imports resolve beside their importing source. `/foo.capnp` searches
/// the configured virtual import roots in insertion order. There are no implicit
/// standard import directories. Each call builds a fresh graph, so failure does
/// not leave stale schemas or alias bindings in subsequent calls.
///
/// ```
/// let mut parser = capnp_compiler::SchemaParser::new();
/// parser.add_source("main.capnp", r#"
///     using Types = import "types.capnp";
///     @0xaaaaaaaaaaaaaaaa;
///     struct Message { item @0 :Types.Item; }
/// "#)?;
/// parser.add_source("types.capnp", "@0xbbbbbbbbbbbbbbbb; struct Item {}")?;
/// let request = parser.parse(&["main.capnp", "types.capnp"])?;
/// let root = request.get_root_as_reader::<
///     capnp::schema_capnp::code_generator_request::Reader<'_>>()?;
/// assert_eq!(root.get_requested_files()?.len(), 2);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Default)]
pub struct SchemaParser {
    sources: BTreeMap<String, Vec<u8>>,
    import_paths: Vec<String>,
    allow_missing_ids: bool,
}

impl SchemaParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Require explicit file IDs (the default), or generate random IDs for files
    /// that omit them. Explicit IDs are always preserved. Generated IDs and their
    /// derived child IDs persist within a session, but change in fresh parses.
    /// Use explicit IDs for persisted schemas and cross-process RPC interfaces.
    /// OS randomness failures are returned as diagnostics; sources are not edited.
    ///
    /// ```
    /// let mut parser = capnp_compiler::SchemaParser::new();
    /// parser.set_file_ids_required(false);
    /// parser.add_source("config.capnp", "struct Config { port @0 :UInt16 = 80; }")?;
    /// let parsed = parser.parse_schemas(&["config.capnp"])?;
    /// assert!(parsed.get_file("config.capnp")?.schema().id() >> 63 != 0);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn set_file_ids_required(&mut self, required: bool) -> &mut Self {
        self.allow_missing_ids = !required;
        self
    }

    /// Register a source under a relative virtual filename. Replacing a source
    /// requires a new parser; accidental duplicate registrations are rejected.
    pub fn add_source(
        &mut self,
        filename: &str,
        text: impl Into<String>,
    ) -> Result<&mut Self, Diagnostic> {
        self.add_file(filename, text.into().into_bytes())
    }

    /// Register a file's exact bytes for `embed` expressions. Files share the
    /// same virtual namespace as sources; UTF-8 files may also be imported as
    /// schemas. Binary files are decoded as UTF-8 only when used as schemas.
    /// Each registered file is limited to 4 MiB.
    pub fn add_file(
        &mut self,
        filename: &str,
        bytes: impl Into<Vec<u8>>,
    ) -> Result<&mut Self, Diagnostic> {
        if filename.starts_with('/') {
            return Err(diagnostic(
                filename,
                "virtual source names must be relative",
            ));
        }
        let path = normalize(filename, false).map_err(|e| diagnostic(filename, e))?;
        if path.is_empty() {
            return Err(diagnostic(filename, "invalid logical filename"));
        }
        if self.sources.contains_key(&path) {
            return Err(diagnostic(filename, "source already registered"));
        }
        let bytes = bytes.into();
        if bytes.len() > MAX_SOURCE_BYTES {
            return Err(diagnostic(filename, "source exceeds 4 MiB limit"));
        }
        self.sources.insert(path, bytes);
        Ok(self)
    }

    /// Add a virtual directory to the search path for slash-prefixed imports.
    /// `.` names the virtual root.
    pub fn import_path(&mut self, path: &str) -> Result<&mut Self, Diagnostic> {
        if path.starts_with('/') {
            return Err(diagnostic(path, "virtual import roots must be relative"));
        }
        self.import_paths
            .push(normalize(path, false).map_err(|e| diagnostic(path, e))?);
        Ok(self)
    }

    /// Produce a CodeGeneratorRequest. Only requested files are marked for code
    /// generation; imported type/annotation dependencies and their enclosing nodes are
    /// included automatically. Repeated requested identities are deduplicated.
    /// Type/alias and annotation-name imports are loaded for request metadata;
    /// value-only and transitive imports load on demand during resolution. Unused
    /// transitive dependencies and embedded files are not opened.
    /// Limits apply to the entire graph: 256 schemas and 256 embedded files,
    /// 16 MiB of combined source/embed input, 4,096 nodes,
    /// 16,384 aliases, 16,384 import edges and 8 MiB of expanded display names.
    /// Resolution has a shared budget of 1,000,000 type/value-expression visits.
    /// Constant/default/annotation payload expansion and encoded storage are each limited
    /// to 16 MiB. Composite depth and work limits also apply; see [`crate::compile`].
    /// Requests include documentation/source ranges for selected nodes and members,
    /// with a separate 16 MiB budget for expanded documentation text.
    /// Each file retains the lexer/parser limits described on [`crate::compile`].
    pub fn parse(&self, requested: &[&str]) -> Result<Message, Diagnostic> {
        compile_graph(self, requested, self.allow_missing_ids)
    }
}

/// Filesystem entry point for the Rust frontend. Files are identified by their
/// canonical paths, so repeated imports and symlink aliases share one schema.
/// This is a compiler, not a filesystem sandbox: relative `..` imports may read
/// outside the source directory. Imports loaded through an import root cannot
/// escape it with `..`. Absolute schema imports search only explicit
/// `import_path()` directories, in order. Embedded files follow the same rules.
#[derive(Default)]
pub struct FileCompiler {
    import_paths: Vec<PathBuf>,
    source_prefix: Option<PathBuf>,
    allow_missing_ids: bool,
}

/// A compiler request and the filesystem inputs used to produce it.
pub struct FileCompilation {
    /// Standard CodeGeneratorRequest message for compiler plugins.
    pub message: Message,
    /// Sorted, unique absolute paths for loaded schemas and embeds, including
    /// symlink spellings and their canonical targets. Unused lazy imports are
    /// excluded. Watch import directories separately if changes to search-path
    /// precedence should trigger a rebuild.
    pub dependencies: Vec<PathBuf>,
}

impl FileCompiler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Choose the same file-ID policy as [`SchemaParser::set_file_ids_required`].
    /// Defaults to requiring explicit IDs. A session captures the current policy;
    /// future configuration changes do not change existing sessions or IDs.
    pub fn set_file_ids_required(&mut self, required: bool) -> &mut Self {
        self.allow_missing_ids = !required;
        self
    }

    pub fn import_path(&mut self, path: impl AsRef<Path>) -> &mut Self {
        self.import_paths.push(path.as_ref().to_owned());
        self
    }

    /// Remove this physical directory prefix from requested output filenames.
    /// Defaults to the working directory. Requested files outside that directory
    /// must use an explicit prefix that contains them.
    pub fn src_prefix(&mut self, path: impl AsRef<Path>) -> &mut Self {
        self.source_prefix = Some(path.as_ref().to_owned());
        self
    }

    pub fn compile(&self, requested: &[impl AsRef<Path>]) -> Result<Message, Diagnostic> {
        Ok(self.compile_with_dependencies(requested)?.message)
    }

    /// Compile and report loaded inputs for build systems such as Cargo.
    /// Each invocation discovers its own dependencies; failures retain no state.
    pub fn compile_with_dependencies(
        &self,
        requested: &[impl AsRef<Path>],
    ) -> Result<FileCompilation, Diagnostic> {
        let loader = self.disk()?;
        let paths = requested_paths(requested)?;
        let message = compile_graph(&loader, &paths, self.allow_missing_ids)?;
        Ok(FileCompilation {
            message,
            dependencies: loader.dependencies.into_inner().into_iter().collect(),
        })
    }

    pub(crate) fn capture_paths(&mut self) -> Result<(), Diagnostic> {
        let disk = self.disk()?;
        self.source_prefix = Some(disk.prefix);
        self.import_paths = disk.import_paths;
        Ok(())
    }

    fn disk(&self) -> Result<Disk, Diagnostic> {
        let prefix = self
            .source_prefix
            .clone()
            .unwrap_or_else(|| PathBuf::from("."));
        let prefix = prefix
            .canonicalize()
            .map_err(|e| diagnostic(&prefix.to_string_lossy(), e.to_string()))?;
        Ok(Disk {
            prefix,
            import_paths: self
                .import_paths
                .iter()
                .map(|path| {
                    // An empty import root historically names the working directory.
                    let path = if path.as_os_str().is_empty() {
                        Path::new(".")
                    } else {
                        path.as_path()
                    };
                    std::path::absolute(path)
                        .map_err(|e| diagnostic(&path.to_string_lossy(), e.to_string()))
                })
                .collect::<Result<_, _>>()?,
            dependencies: RefCell::new(BTreeSet::new()),
        })
    }
}

fn requested_paths(requested: &[impl AsRef<Path>]) -> Result<Vec<&str>, Diagnostic> {
    requested
        .iter()
        .map(|path| {
            path.as_ref()
                .to_str()
                .ok_or_else(|| diagnostic("<path>", "requested paths must be UTF-8"))
        })
        .collect()
}

struct Loaded {
    rooted: bool,
    identity: String,
    filename: String,
    bytes: Vec<u8>,
}

trait Loader {
    fn load(
        &self,
        from: Option<&File>,
        path: &str,
        known: &BTreeMap<String, usize>,
    ) -> Result<Loaded, String>;
}

impl Loader for SchemaParser {
    fn load(
        &self,
        from: Option<&File>,
        path: &str,
        known: &BTreeMap<String, usize>,
    ) -> Result<Loaded, String> {
        let candidates = match from {
            None => {
                if path.starts_with('/') {
                    return Err("requested virtual filenames must be relative".into());
                }
                let key = normalize(path, false)?;
                vec![(key.clone(), key)]
            }
            Some(_) if path.starts_with('/') => {
                let name = normalize(&path[1..], false)?;
                self.import_paths
                    .iter()
                    .map(|root| (format!("{root}/{name}"), name.clone()))
                    .collect()
            }
            Some(from) => vec![(
                relative_to(&from.identity, path, false)?,
                relative_to(&from.filename, path, !from.rooted)?,
            )],
        };
        for (key, filename) in candidates {
            let key = normalize(&key, false)?;
            if let Some(bytes) = self.sources.get(&key) {
                let bytes = if known.contains_key(&key) {
                    Vec::new()
                } else {
                    bytes.clone()
                };
                return Ok(Loaded {
                    rooted: from.is_some_and(|f| f.rooted || path.starts_with('/')),
                    identity: key,
                    filename,
                    bytes,
                });
            }
        }
        Err(format!("schema file not found: {path}"))
    }
}

#[derive(Clone)]
struct Disk {
    prefix: PathBuf,
    import_paths: Vec<PathBuf>,
    dependencies: RefCell<BTreeSet<PathBuf>>,
}

impl Loader for Disk {
    fn load(
        &self,
        from: Option<&File>,
        path: &str,
        known: &BTreeMap<String, usize>,
    ) -> Result<Loaded, String> {
        let candidates = match from {
            None => vec![(PathBuf::from(path), None)],
            Some(_) if path.starts_with('/') => {
                let name = normalize(&path[1..], false)?;
                self.import_paths
                    .iter()
                    .map(|root| (root.join(&name), Some(name.clone())))
                    .collect()
            }
            Some(from) => {
                // Normalize the import itself first to reject host-specific path
                // prefixes before passing it to Path::join on Windows.
                let import = normalize(path, true)?;
                let parent = Path::new(&from.identity)
                    .parent()
                    .ok_or("importer has no parent directory")?;
                vec![(
                    parent.join(import),
                    Some(relative_to(&from.filename, path, !from.rooted)?),
                )]
            }
        };
        for (candidate, display) in candidates {
            let physical = match candidate.canonicalize() {
                Ok(path) => path,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(format!("{}: {e}", candidate.display())),
            };
            let mut dependencies = self.dependencies.borrow_mut();
            dependencies.insert(std::path::absolute(&candidate).map_err(|e| e.to_string())?);
            dependencies.insert(physical.clone());
            drop(dependencies);
            let identity = physical
                .to_str()
                .ok_or("schema paths must be UTF-8")?
                .to_owned();
            let filename = match display {
                Some(name) => name,
                None => physical
                    .strip_prefix(&self.prefix)
                    .map_err(|_| "requested file is outside --src-prefix")?
                    .to_str()
                    .ok_or("schema paths must be UTF-8")?
                    .replace('\\', "/"),
            };
            if known.contains_key(&identity) {
                return Ok(Loaded {
                    rooted: from.is_some_and(|f| f.rooted || path.starts_with('/')),
                    identity,
                    filename,
                    bytes: Vec::new(),
                });
            }
            if !physical.is_file() {
                return Err(format!("not a regular file: {identity}"));
            }
            let mut bytes = Vec::new();
            std::fs::File::open(&physical)
                .map_err(|e| format!("{identity}: {e}"))?
                .take(MAX_SOURCE_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| format!("{identity}: {e}"))?;
            return Ok(Loaded {
                rooted: from.is_some_and(|f| f.rooted || path.starts_with('/')),
                identity,
                filename,
                bytes,
            });
        }
        Err(format!("schema file not found: {path}"))
    }
}

#[derive(Clone)]
pub(crate) struct Import {
    pub metadata: bool,
    pub path: String,
    pub span: Span,
    pub target: usize, // File index, not node index.
}

#[derive(Clone)]
pub(crate) struct File {
    rooted: bool,
    identity: String,
    pub filename: String,
    text: String,
    pub root: usize,
    pub imports: Vec<Import>,
}

impl File {
    pub fn source(&self) -> Source<'_> {
        Source {
            filename: &self.filename,
            text: &self.text,
        }
    }
}

#[derive(Clone)]
pub(crate) struct Graph {
    allow_missing_ids: bool,
    pub files: Vec<File>,
    pub nodes: Vec<Node>,
    pub requested: Vec<usize>,
    identities: BTreeMap<String, usize>,
    ids: BTreeMap<u64, usize>,
    total_bytes: usize,
    total_aliases: usize,
    total_imports: usize,
    pub embeds: BTreeMap<(usize, String), Arc<crate::embed::File>>,
    embed_identities: BTreeMap<String, usize>,
    embed_data: Vec<Arc<crate::embed::File>>,
}

impl Graph {
    pub fn source(&self, node: usize) -> Source<'_> {
        self.files[self.nodes[node].file].source()
    }

    fn insert(&mut self, input: Loaded) -> Result<usize, Diagnostic> {
        if let Some(&index) = self.identities.get(&input.identity) {
            return Ok(index);
        }
        if input.bytes.len() > MAX_SOURCE_BYTES {
            return Err(diagnostic(&input.filename, "source exceeds 4 MiB limit"));
        }
        let text = String::from_utf8(input.bytes)
            .map_err(|_| diagnostic(&input.filename, "schema source must be UTF-8"))?;
        let source = Source {
            filename: &input.filename,
            text: &text,
        };
        let error = |message| source.error(Span::default(), message);
        if input.filename.is_empty() || input.filename.len() > 4096 || input.filename.contains('\0')
        {
            return Err(error("invalid logical filename"));
        }
        if self.files.len() == MAX_FILES {
            return Err(error("import file limit exceeded (256)"));
        }
        self.total_bytes += text.len();
        if self.total_bytes > MAX_TOTAL_BYTES {
            return Err(error("total imported source/embed limit exceeded (16 MiB)"));
        }
        let parsed = syntax::parse(&source, self.allow_missing_ids)?;
        if self.nodes.len() + parsed.nodes.len() > 4096 {
            return Err(error("total node limit exceeded (4096)"));
        }
        self.total_aliases += parsed.nodes.iter().map(|n| n.aliases.len()).sum::<usize>();
        self.total_imports += parsed.imports.len();
        if self.total_aliases > 16384 || self.total_imports > 16384 {
            return Err(error("alias/import count limit exceeded (16384)"));
        }
        let file = self.files.len();
        let root = self.nodes.len();
        for mut node in parsed.nodes {
            if let Some(&previous) = self.ids.get(&node.id) {
                return Err(source.error(
                    node.span,
                    format!(
                        "duplicate schema ID 0x{:016x}; already declared in {}",
                        node.id, self.files[previous].filename
                    ),
                ));
            }
            self.ids.insert(node.id, file);
            node.parent = node.parent.map(|p| p + root);
            node.children.iter_mut().for_each(|i| *i += root);
            if let crate::syntax::NodeKind::Struct(fields) = &mut node.kind {
                for field in fields {
                    if let crate::syntax::FieldKind::Group(group) = &mut field.kind {
                        *group += root;
                    }
                }
            }
            if let crate::syntax::NodeKind::Interface { methods, .. } = &mut node.kind {
                for method in methods {
                    for params in [&mut method.params, &mut method.results] {
                        if let crate::syntax::ParamList::Inline(index) = params {
                            *index += root;
                        }
                    }
                }
            }
            node.file = file;
            self.nodes.push(node);
        }
        self.identities.insert(input.identity.clone(), file);
        self.files.push(File {
            rooted: input.rooted,
            identity: input.identity,
            filename: input.filename,
            text,
            root,
            imports: parsed
                .imports
                .into_iter()
                .map(|i| Import {
                    metadata: i.metadata,
                    path: i.path,
                    span: i.span,
                    target: usize::MAX,
                })
                .collect(),
        });
        Ok(file)
    }
}

fn compile_graph(
    loader: &impl Loader,
    requested: &[&str],
    allow_missing_ids: bool,
) -> Result<Message, Diagnostic> {
    let mut graph = read_graph(loader, requested, allow_missing_ids)?;
    compile_loaded_graph(loader, &mut graph, &BTreeSet::new())
}

fn read_graph(
    loader: &impl Loader,
    requested: &[&str],
    allow_missing_ids: bool,
) -> Result<Graph, Diagnostic> {
    if requested.is_empty() {
        return Err(diagnostic(
            "<input>",
            "at least one requested file is required",
        ));
    }
    let mut graph = Graph {
        allow_missing_ids,
        files: vec![],
        nodes: vec![],
        requested: vec![],
        identities: BTreeMap::new(),
        ids: BTreeMap::new(),
        total_bytes: 0,
        total_aliases: 0,
        total_imports: 0,
        embeds: BTreeMap::new(),
        embed_identities: BTreeMap::new(),
        embed_data: Vec::new(),
    };
    for path in requested {
        let file = graph.insert(
            loader
                .load(None, path, &graph.identities)
                .map_err(|e| diagnostic(path, e))?,
        )?;
        if !graph.requested.contains(&file) {
            graph.requested.push(file);
        }
    }
    Ok(graph)
}

fn compile_loaded_graph(
    loader: &impl Loader,
    graph: &mut Graph,
    additional: &BTreeSet<usize>,
) -> Result<Message, Diagnostic> {
    let mut budget = 1_000_000;
    loop {
        match crate::resolve::compile(graph, &mut budget, additional) {
            Ok(compiled) => {
                // C++ emits the import table after compiling the requested
                // declarations. Unused metadata must not choose a file's first
                // display spelling ahead of an actual dependency.
                let mut missing = Vec::new();
                for &file in &graph.requested {
                    for (index, import) in graph.files[file].imports.iter().enumerate() {
                        if import.metadata && import.target == usize::MAX {
                            missing.push((file, import.path.clone(), index));
                        }
                    }
                }
                if missing.is_empty() {
                    return crate::emit::compile(graph, &compiled);
                }
                missing.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
                for (file, _, index) in missing {
                    load_import(loader, graph, file, index)?;
                }
                // New syntax changes node/identifier table sizes, even when
                // the imported declarations themselves remain unselected.
            }
            Err(failure) => discover(loader, graph, failure)?,
        }
    }
}

fn discover(
    loader: &impl Loader,
    graph: &mut Graph,
    failure: crate::resolve::Failure,
) -> Result<(), Diagnostic> {
    match failure {
        crate::resolve::Failure::Diagnostic(error) => return Err(error),
        crate::resolve::Failure::Embed { file, path, span } => {
            let from = &graph.files[file];
            let error = |m| from.source().error(span, m);
            let input = loader
                .load(Some(from), &path, &graph.embed_identities)
                .map_err(error)?;
            let data = if let Some(&index) = graph.embed_identities.get(&input.identity) {
                graph.embed_data[index].clone()
            } else {
                if input.bytes.len() > MAX_SOURCE_BYTES {
                    return Err(error("embedded file exceeds 4 MiB limit".into()));
                }
                if graph.embed_data.len() == MAX_FILES {
                    return Err(error("embedded file limit exceeded (256)".into()));
                }
                graph.total_bytes += input.bytes.len();
                if graph.total_bytes > MAX_TOTAL_BYTES {
                    return Err(error(
                        "total source/embed size limit exceeded (16 MiB)".into(),
                    ));
                }
                let data = Arc::new(crate::embed::File::new(input.bytes));
                graph
                    .embed_identities
                    .insert(input.identity, graph.embed_data.len());
                graph.embed_data.push(data.clone());
                data
            };
            graph.embeds.insert((file, path), data);
        }
        crate::resolve::Failure::Import { file, index } => {
            load_import(loader, graph, file, index)?;
        }
    }
    Ok(())
}

#[derive(Clone)]
enum SessionInput<'a> {
    Memory(&'a SchemaParser),
    Disk(Disk),
    Custom(custom::Input<'a>),
}

impl Loader for SessionInput<'_> {
    fn load(
        &self,
        from: Option<&File>,
        path: &str,
        known: &BTreeMap<String, usize>,
    ) -> Result<Loaded, String> {
        match self {
            Self::Memory(parser) => parser.load(from, path, known),
            Self::Disk(disk) => disk.load(from, path, known),
            Self::Custom(input) => input.load(from, path, known),
        }
    }
}

/// Staged source state. Clone before an extension; commit only after compilation
/// and runtime validation succeed. Previously read schemas/embeds stay cached.
#[derive(Clone)]
pub(crate) struct Session<'a> {
    input: SessionInput<'a>,
    graph: Graph,
    additional: BTreeSet<usize>,
}

impl<'a> Session<'a> {
    pub fn custom(compiler: &SourceCompiler<'a>, requested: &[&str]) -> Result<Self, Diagnostic> {
        let input = SessionInput::Custom(custom::Input(compiler.provider));
        let graph = read_graph(&input, requested, compiler.allow_missing_ids)?;
        Ok(Self {
            input,
            graph,
            additional: BTreeSet::new(),
        })
    }

    pub fn memory(parser: &'a SchemaParser, requested: &[&str]) -> Result<Self, Diagnostic> {
        let input = SessionInput::Memory(parser);
        let graph = read_graph(&input, requested, parser.allow_missing_ids)?;
        Ok(Self {
            input,
            graph,
            additional: BTreeSet::new(),
        })
    }

    pub fn disk(
        compiler: &FileCompiler,
        requested: &[impl AsRef<Path>],
    ) -> Result<Session<'static>, Diagnostic> {
        let input = SessionInput::Disk(compiler.disk()?);
        let graph = read_graph(
            &input,
            &requested_paths(requested)?,
            compiler.allow_missing_ids,
        )?;
        Ok(Session {
            input,
            graph,
            additional: BTreeSet::new(),
        })
    }

    pub fn compile(&mut self) -> Result<FileCompilation, Diagnostic> {
        let message = compile_loaded_graph(&self.input, &mut self.graph, &self.additional)?;
        let dependencies = match &self.input {
            SessionInput::Memory(_) | SessionInput::Custom(_) => Vec::new(),
            SessionInput::Disk(disk) => disk.dependencies.borrow().iter().cloned().collect(),
        };
        Ok(FileCompilation {
            message,
            dependencies,
        })
    }

    fn index(&self, id: u64) -> Result<usize, Diagnostic> {
        self.graph
            .nodes
            .iter()
            .position(|node| node.id == id)
            .ok_or_else(|| diagnostic("<lookup>", format!("unknown schema ID {id:#x}")))
    }

    pub fn children(&self, id: u64) -> Result<Vec<(String, u64)>, Diagnostic> {
        let index = self.index(id)?;
        Ok(self.graph.nodes[index]
            .children
            .iter()
            .map(|&child| {
                let node = &self.graph.nodes[child];
                (node.name.clone(), node.id)
            })
            .collect())
    }

    pub fn require(&mut self, id: u64) -> Result<(), Diagnostic> {
        let index = self.index(id)?;
        self.additional.insert(index);
        Ok(())
    }

    pub fn has_alias(&self, id: u64, name: &str) -> Result<bool, Diagnostic> {
        Ok(self.graph.nodes[self.index(id)?]
            .aliases
            .iter()
            .any(|alias| alias.name == name))
    }

    pub fn lookup(&mut self, id: u64, name: &str) -> Result<Option<u64>, Diagnostic> {
        let scope = self.index(id)?;
        let mut budget = 1_000_000;
        loop {
            match crate::resolve::lookup(&self.graph, scope, name, &mut budget) {
                Ok(index) => return Ok(index.map(|index| self.graph.nodes[index].id)),
                Err(failure) => discover(&self.input, &mut self.graph, failure)?,
            }
        }
    }
}

fn load_import(
    loader: &impl Loader,
    graph: &mut Graph,
    file: usize,
    index: usize,
) -> Result<(), Diagnostic> {
    let from = &graph.files[file];
    let import = &from.imports[index];
    let input = loader
        .load(Some(from), &import.path, &graph.identities)
        .map_err(|e| from.source().error(import.span, e))?;
    let target = graph.insert(input)?;
    graph.files[file].imports[index].target = target;
    Ok(())
}
