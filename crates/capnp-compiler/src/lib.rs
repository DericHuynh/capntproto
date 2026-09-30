//! Compile a supported subset of the Cap'n Proto schema language entirely in Rust.
//!
//! The output is a standard `schema::CodeGeneratorRequest`, usable by `capnpc`
//! and other compiler plugins. The in-memory [`SchemaParser`] and filesystem
//! [`FileCompiler`] share the same frontend; neither invokes subprocesses.
//! Their `parse_schemas()` methods also return validated [`ParsedSchemas`]
//! snapshots for runtime reflection and dynamic messages.
//! [`SchemaSession`] adds explicit lazy declaration loading under an exclusive borrow.
//! [`ConcurrentSchemaParser`] shares a worker-owned session and immutable snapshots
//! between threads; reflection views are materialized locally.
//! [`SourceCompiler`] accepts custom resolution and bounded I/O through [`SourceProvider`].

#![forbid(unsafe_code)]

mod brand;
mod cache;
mod embed;
mod emit;
mod layout;
mod parsed;
mod resolve;
mod session;
mod source;
mod source_info;
mod syntax;
mod value;
mod wire;

pub use cache::{CachedSchema, CachedSchemas, ConcurrentSchemaParser};
pub use parsed::{ParseError, ParsedSchema, ParsedSchemas};
pub use session::SchemaSession;
pub mod literal;
pub use source::{
    FileCompilation, FileCompiler, SchemaParser, SourceCompiler, SourceFile, SourceProvider,
};

use std::fmt;

/// Maximum UTF-8 source length accepted per source file (4 MiB).
pub const MAX_SOURCE_BYTES: usize = 4 * 1024 * 1024;

/// A source diagnostic. Offsets are UTF-8 byte offsets; line and column are
/// one-based, with columns counting Unicode scalar values rather than bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub filename: String,
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub column: usize,
    pub message: String,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}: {}",
            self.filename, self.line, self.column, self.message
        )
    }
}

impl std::error::Error for Diagnostic {}

pub(crate) struct Source<'a> {
    filename: &'a str,
    text: &'a str,
}

impl Source<'_> {
    fn error(&self, span: syntax::Span, message: impl Into<String>) -> Diagnostic {
        let prefix = &self.text[..span.start];
        Diagnostic {
            filename: self.filename.into(),
            start: span.start,
            end: span.end,
            line: prefix.bytes().filter(|&b| b == b'\n').count() + 1,
            column: prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1,
            message: message.into(),
        }
    }
}

/// Compile one schema file, returning an unframed message builder whose root is
/// `capnp::schema_capnp::code_generator_request`.
///
/// `filename` is the logical filename recorded in the request; it is not opened.
/// It must be a relative, slash-separated virtual path. For imports, use
/// [`SchemaParser`] to register the other files or [`FileCompiler`] to read them.
/// Embedded files use the same APIs; [`SchemaParser::add_file`] accepts raw bytes.
/// When passing the request to a code generator, the caller controls this name
/// and the generator's output directory. Use `capnp::serialize::write_message`
/// to produce the standard compiler-plugin stream.
///
/// Compilation is all-or-error. Unsupported constructs are never skipped.
/// Limits: 4 MiB of source, 262,144 tokens, 4,096 nodes (including the file),
/// 64 levels of combined declaration/type nesting, and 65,535 explicit ordinals
/// per struct (shared with its groups), enum or interface. Inline method parameter
/// and result structs count toward the node limit. Expanded display names are
/// limited to 8 MiB; wire layout has a shared budget of 1,000,000 allocation and
/// location-search steps. Constants, defaults and annotation values share a 16 MiB payload
/// expansion limit and a separate 16 MiB encoded-storage budget. Composite
/// expansion is depth-limited to 64, with 1,000,000 expanded value visits and
/// a separate shared budget of 1,000,000 encoding steps, including group clears.
/// Historical issue #344 layouts rejected by C++
/// also return diagnostics here.
/// Requests include documentation comments and node/member byte ranges.
/// Expanded documentation text has a separate 16 MiB output budget.
///
/// ```
/// let request = capnp_compiler::compile(
///     "point.capnp",
///     "@0xdeadbeefdeadbeef; struct Point { x @0 :Int32; y @1 :Int32; }",
/// )?;
/// let root = request.get_root_as_reader::<
///     capnp::schema_capnp::code_generator_request::Reader<'_>>()?;
/// assert_eq!(root.get_nodes()?.len(), 2);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn compile(
    filename: &str,
    text: &str,
) -> Result<capnp::message::Builder<capnp::message::HeapAllocator>, Diagnostic> {
    let source = Source { filename, text };
    if text.len() > MAX_SOURCE_BYTES {
        return Err(source.error(syntax::Span::default(), "source exceeds 4 MiB limit"));
    }
    if filename.is_empty() || filename.len() > 4096 || filename.contains('\0') {
        return Err(source.error(syntax::Span::default(), "invalid logical filename"));
    }
    let mut parser = SchemaParser::new();
    parser.add_source(filename, text)?;
    parser.parse(&[filename])
}
