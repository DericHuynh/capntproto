//! Concurrent access to a single transactional parser session.
use crate::{
    FileCompiler, ParseError, ParsedSchemas, SchemaParser, SchemaSession, SourceCompiler,
    SourceProvider,
};
use capnp::{message, schema_loader::Limits};
use std::{
    path::{Path, PathBuf},
    sync::{
        mpsc::{self, Receiver, SyncSender},
        Arc,
    },
    thread::{self, JoinHandle, ThreadId},
};

type Result<T> = std::result::Result<T, ParseError>;

fn unavailable(message: &str) -> ParseError {
    capnp::Error::failed(message.into()).into()
}

/// An immutable, thread-safe compiled snapshot. Cloning shares its storage.
///
/// This contains a validated serialized request, not thread-local runtime
/// reflection objects. [`Self::materialize`] creates those on the calling thread.
/// Retained snapshots never change when the cache discovers more declarations.
#[derive(Clone)]
pub struct CachedSchemas(Arc<Snapshot>);

struct Snapshot {
    request: Vec<u8>,
    dependencies: Vec<PathBuf>,
    limits: Limits,
    revision: u64,
    requested: Vec<(String, u64)>,
}

impl CachedSchemas {
    fn new(session: &SchemaSession<'_>, limits: Limits) -> Self {
        Self(Arc::new(Snapshot {
            request: session.schemas().serialized_request(),
            dependencies: session.schemas().dependencies().to_vec(),
            limits,
            revision: session.revision(),
            requested: session
                .schemas()
                .requested_files()
                .map(|(name, schema)| (name.to_owned(), schema.schema().id()))
                .collect(),
        }))
    }

    /// Generation within this cache, starting at zero. Failed operations, missing
    /// names and cache hits do not change it. Not an ID across different caches.
    pub fn revision(&self) -> u64 {
        self.0.revision
    }

    /// The framed, uncompressed CodeGeneratorRequest, suitable for a generator.
    pub fn serialized_request(&self) -> &[u8] {
        &self.0.request
    }

    /// Successfully loaded disk inputs at this generation; empty for providers
    /// and in-memory sources. Retained snapshots keep their original list.
    pub fn dependencies(&self) -> &[PathBuf] {
        &self.0.dependencies
    }

    /// Requested files in original order, including generated file IDs.
    pub fn requested_files(&self) -> impl ExactSizeIterator<Item = (&str, CachedSchema)> {
        self.0.requested.iter().map(|(name, id)| {
            (
                name.as_str(),
                CachedSchema {
                    schemas: self.clone(),
                    id: *id,
                },
            )
        })
    }

    pub fn get_file(&self, filename: &str) -> Result<CachedSchema> {
        self.requested_files()
            .find(|(name, _)| *name == filename)
            .map(|(_, schema)| schema)
            .ok_or_else(|| unavailable(&format!("file was not requested: {filename}")))
    }

    /// Create independent local reflection objects using this cache's loader
    /// limits. This copies and validates the request; it performs no source I/O
    /// or textual compilation. Retain the result for repeated dynamic access.
    pub fn materialize(&self) -> Result<ParsedSchemas> {
        // Bytes can only originate from our bounded, already validated frontend.
        // Avoid the binary reader's unrelated default traversal budget on copies.
        let reader = capnp::serialize::read_message(
            self.0.request.as_slice(),
            message::ReaderOptions {
                traversal_limit_in_words: None,
                nesting_limit: i32::MAX,
            },
        )?;
        let mut message = message::Builder::new_default();
        message.set_root(
            reader.get_root::<capnp::schema_capnp::code_generator_request::Reader<'_>>()?,
        )?;
        ParsedSchemas::load(message, self.0.dependencies.clone(), self.0.limits)
    }
}

/// A declaration ID paired with the immutable generation that compiled it.
/// Clone or send this handle across threads; borrow a runtime schema from
/// `handle.schemas().materialize()?.get(handle.id())` on the consuming thread.
#[derive(Clone)]
pub struct CachedSchema {
    schemas: CachedSchemas,
    id: u64,
}
impl CachedSchema {
    pub fn id(&self) -> u64 {
        self.id
    }
    pub fn schemas(&self) -> &CachedSchemas {
        &self.schemas
    }
}

enum Query {
    Snapshot,
    Load(u64),
    Find(u64, String),
    All(u64),
}
struct Reply {
    schemas: CachedSchemas,
    ids: Vec<u64>,
}
struct Job {
    query: Query,
    reply: SyncSender<Result<Reply>>,
}
struct Client {
    sender: Option<SyncSender<Job>>,
    worker: Option<JoinHandle<()>>,
    worker_id: ThreadId,
}
impl Drop for Client {
    fn drop(&mut self) {
        // Closing the last sender wakes an idle worker before joining it.
        drop(self.sender.take());
        if let Some(worker) = self.worker.take() {
            if thread::current().id() != self.worker_id {
                let _ = worker.join();
            }
        }
    }
}

/// Cloneable, thread-safe access to one lazy schema compilation cache.
///
/// A dedicated worker owns the session and serializes loads, so concurrent
/// requests share successful source reads, generated file IDs and compilation.
/// A bounded queue (16 waiting operations) applies backpressure. Methods block;
/// use a blocking executor when calling from async code. Different caches are
/// independent. Dropping the last client closes the queue and joins the worker.
///
/// Returned handles own immutable snapshots and outlive the cache. Extensions
/// publish only after complete compilation and runtime validation; errors leave
/// the prior generation intact and can be retried. Successful source bytes stay
/// cached: create a fresh cache to observe changes. No automatic invalidation or
/// global cache is used. Limits and requested-file order match [`SchemaSession`].
///
/// Runtime reflection remains local: materialize a snapshot on each thread that
/// needs it. This does not make `capnp::schema_loader::SchemaLoader` thread-safe.
/// A panicking provider stops the worker; callers receive an error. Provider
/// callbacks must not reenter the same cache (detected and rejected) or wait for
/// another thread that is itself waiting for this cache.
///
/// ```
/// let mut parser = capntproto_compiler::SchemaParser::new();
/// parser.add_source("main.capnp", "@0xaaaaaaaaaaaaaaaa; using D = import \"types.capnp\"; struct Root { x @0 :D.Used; }")?;
/// parser.add_source("types.capnp", "@0xbbbbbbbbbbbbbbbb; struct Used {} struct Later { value @0 :Text; }")?;
/// let cache = parser.into_concurrent(&["main.capnp"])?;
/// let other = cache.clone();
/// let handle = std::thread::spawn(move || other.get_nested(0xbbbbbbbbbbbbbbbb, "Later")).join().unwrap()?;
/// drop(cache);
/// let local = handle.schemas().materialize()?;
/// assert!(local.get(handle.id())?.schema().field("value").is_ok());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone)]
pub struct ConcurrentSchemaParser(Arc<Client>);

impl ConcurrentSchemaParser {
    fn start(
        run: impl FnOnce(Receiver<Job>, SyncSender<Result<CachedSchemas>>) + Send + 'static,
    ) -> Result<Self> {
        let (sender, receiver) = mpsc::sync_channel(16);
        let (ready, initial) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("capnp-schema-cache".into())
            .spawn(move || run(receiver, ready))
            .map_err(|e| unavailable(&format!("could not start schema cache: {e}")))?;
        let client = Client {
            worker_id: worker.thread().id(),
            sender: Some(sender),
            worker: Some(worker),
        };
        initial
            .recv()
            .map_err(|_| unavailable("schema cache worker stopped during initialization"))??;
        Ok(Self(Arc::new(client)))
    }

    /// Own a source provider on the worker. Only `Send` is required; callbacks
    /// never run concurrently, and their borrowed readers stay on the worker.
    pub fn from_provider(
        provider: impl SourceProvider + Send + 'static,
        requested: &[&str],
    ) -> Result<Self> {
        Self::from_provider_with_options(provider, requested, true, Limits::default())
    }

    /// Like [`Self::from_provider`], selecting file-ID policy and loader limits.
    /// Callback side effects are not rolled back when an operation fails.
    pub fn from_provider_with_options(
        provider: impl SourceProvider + Send + 'static,
        requested: &[&str],
        file_ids_required: bool,
        limits: Limits,
    ) -> Result<Self> {
        let requested: Vec<String> = requested.iter().map(|name| (*name).into()).collect();
        Self::start(move |receiver, ready| {
            let mut compiler = SourceCompiler::new(&provider);
            compiler.set_file_ids_required(file_ids_required);
            let names: Vec<&str> = requested.iter().map(String::as_str).collect();
            serve(
                compiler.parse_session_with_limits(&names, limits),
                limits,
                receiver,
                ready,
            );
        })
    }

    fn query(&self, query: Query) -> Result<Reply> {
        if thread::current().id() == self.0.worker_id {
            return Err(unavailable("schema provider cannot reenter its own cache"));
        }
        let (reply, receive) = mpsc::sync_channel(1);
        self.0
            .sender
            .as_ref()
            .expect("live cache client")
            .send(Job { query, reply })
            .map_err(|_| unavailable("schema cache worker stopped"))?;
        receive
            .recv()
            .map_err(|_| unavailable("schema cache worker stopped"))?
    }

    /// Obtain the latest complete generation without compiling declarations.
    pub fn schemas(&self) -> Result<CachedSchemas> {
        Ok(self.query(Query::Snapshot)?.schemas)
    }

    pub fn load(&self, id: u64) -> Result<CachedSchema> {
        let reply = self.query(Query::Load(id))?;
        Ok(CachedSchema {
            schemas: reply.schemas,
            id,
        })
    }

    /// Resolve a declaration or alias, including lazy source discovery. Missing
    /// names return None without I/O. Generic bindings are erased as in sessions.
    pub fn find_nested(&self, parent: u64, name: &str) -> Result<Option<CachedSchema>> {
        // Only declared names can match. Bound queued input independently of
        // compiler graph limits, including malicious calls with huge names.
        if name.len() > crate::MAX_SOURCE_BYTES {
            return Err(unavailable("schema lookup name exceeds source size limit"));
        }
        let reply = self.query(Query::Find(parent, name.into()))?;
        Ok(reply.ids.first().map(|&id| CachedSchema {
            schemas: reply.schemas.clone(),
            id,
        }))
    }

    pub fn get_nested(&self, parent: u64, name: &str) -> Result<CachedSchema> {
        self.find_nested(parent, name)?
            .ok_or_else(|| unavailable(&format!("no such nested declaration: {name}")))
    }

    /// Atomically load all direct declarations in source order, excluding aliases.
    pub fn get_all_nested(&self, parent: u64) -> Result<Vec<CachedSchema>> {
        let reply = self.query(Query::All(parent))?;
        Ok(reply
            .ids
            .into_iter()
            .map(|id| CachedSchema {
                schemas: reply.schemas.clone(),
                id,
            })
            .collect())
    }
}

fn serve(
    session: Result<SchemaSession<'_>>,
    limits: Limits,
    receiver: Receiver<Job>,
    ready: SyncSender<Result<CachedSchemas>>,
) {
    let mut session = match session {
        Ok(session) => session,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    let mut snapshot = CachedSchemas::new(&session, limits);
    if ready.send(Ok(snapshot.clone())).is_err() {
        return;
    }
    for Job { query, reply } in receiver {
        let ids = match query {
            Query::Snapshot => Ok(Vec::new()),
            Query::Load(id) => session.load(id).map(|s| vec![s.schema().id()]),
            Query::Find(parent, name) => session
                .find_nested(parent, &name)
                .map(|s| s.into_iter().map(|s| s.schema().id()).collect()),
            Query::All(parent) => session
                .get_all_nested(parent)
                .map(|schemas| schemas.into_iter().map(|s| s.schema().id()).collect()),
        };
        let result = ids.map(|ids| {
            if snapshot.revision() != session.revision() {
                snapshot = CachedSchemas::new(&session, limits);
            }
            Reply {
                schemas: snapshot.clone(),
                ids,
            }
        });
        let _ = reply.send(result);
    }
}

impl SchemaParser {
    /// Move this virtual filesystem into a concurrent lazy parser cache.
    pub fn into_concurrent(self, requested: &[&str]) -> Result<ConcurrentSchemaParser> {
        self.into_concurrent_with_limits(requested, Limits::default())
    }

    pub fn into_concurrent_with_limits(
        self,
        requested: &[&str],
        limits: Limits,
    ) -> Result<ConcurrentSchemaParser> {
        let requested: Vec<String> = requested.iter().map(|name| (*name).into()).collect();
        ConcurrentSchemaParser::start(move |receiver, ready| {
            let names: Vec<&str> = requested.iter().map(String::as_str).collect();
            serve(
                self.parse_session_with_limits(&names, limits),
                limits,
                receiver,
                ready,
            );
        })
    }
}

impl FileCompiler {
    /// Move this filesystem configuration into a concurrent lazy parser cache.
    /// Paths are captured as absolute paths before starting the worker.
    pub fn into_concurrent(self, requested: &[impl AsRef<Path>]) -> Result<ConcurrentSchemaParser> {
        self.into_concurrent_with_limits(requested, Limits::default())
    }

    pub fn into_concurrent_with_limits(
        mut self,
        requested: &[impl AsRef<Path>],
        limits: Limits,
    ) -> Result<ConcurrentSchemaParser> {
        self.capture_paths()?;
        let requested = requested
            .iter()
            .map(|p| std::path::absolute(p).map_err(|e| unavailable(&e.to_string())))
            .collect::<Result<Vec<_>>>()?;
        ConcurrentSchemaParser::start(move |receiver, ready| {
            serve(
                self.parse_session_with_limits(&requested, limits),
                limits,
                receiver,
                ready,
            );
        })
    }
}
