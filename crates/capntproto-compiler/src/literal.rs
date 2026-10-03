//! Standalone text values. No imports, embeds or constant resolution are performed.
use crate::{Diagnostic, Source};

/// Syntax tree of a single Cap'n Proto text value, before schema type checking.
#[derive(Clone, Debug, PartialEq)]
pub enum Literal {
    Number(String),
    Name(String),
    Text(Vec<u8>),
    Data(Vec<u8>),
    List(Vec<Literal>),
    Struct(Vec<(String, Literal)>),
}

/// Parse exactly one expression using the schema compiler's bounded lexer.
/// Names can designate booleans, special floats, void or enumerants; qualified
/// names, imports and embeds are rejected. The consumer must type-check names.
pub fn parse_literal(input: &str) -> Result<Literal, Diagnostic> {
    let source = Source {
        filename: "(capnp text input)",
        text: input,
    };
    if input.len() > crate::MAX_SOURCE_BYTES {
        return Err(source.error(Default::default(), "text input exceeds 4 MiB"));
    }
    crate::syntax::parse_literal(&source)
}
