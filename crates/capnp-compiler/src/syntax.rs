use crate::{
    source_info::{self, Info},
    Diagnostic, Source,
};
use std::collections::BTreeSet;

const MAX_TOKENS: usize = 262_144;
const MAX_NODES: usize = 4096;
const MAX_DEPTH: usize = 64;

#[cfg(test)]
mod file_id_tests;

type Arguments = Vec<(Option<(String, Span)>, Expr)>;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum TokenKind {
    Name(String),
    Number(String),
    Text(Vec<u8>),
    Data(Vec<u8>),
    Punct(char),
    Arrow,
    End,
}

#[derive(Clone, Debug)]
pub(crate) struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub(crate) enum TypeKind {
    Name { name: String, absolute: bool },
    Import(usize),
    Member(Box<Type>, String),
    Apply(Box<Type>, Vec<Type>),
}

#[derive(Clone, Debug)]
pub(crate) struct Type {
    pub kind: TypeKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub(crate) enum ExprKind {
    Literal(TokenKind),
    Reference(Type),
    Embed(String),
    List(Vec<Expr>),
    Struct(Vec<Assignment>),
}

#[derive(Clone, Debug)]
pub(crate) struct Assignment {
    pub name: String,
    pub span: Span,
    pub value: Expr,
}

#[derive(Clone, Debug)]
pub(crate) struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum AnnotationTarget {
    File,
    Const,
    Enum,
    Enumerant,
    Struct,
    Field,
    Union,
    Group,
    Interface,
    Method,
    Param,
    Annotation,
}

impl AnnotationTarget {
    pub const ALL: [Self; 12] = [
        Self::File,
        Self::Const,
        Self::Enum,
        Self::Enumerant,
        Self::Struct,
        Self::Field,
        Self::Union,
        Self::Group,
        Self::Interface,
        Self::Method,
        Self::Param,
        Self::Annotation,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Const => "const",
            Self::Enum => "enum",
            Self::Enumerant => "enumerant",
            Self::Struct => "struct",
            Self::Field => "field",
            Self::Union => "union",
            Self::Group => "group",
            Self::Interface => "interface",
            Self::Method => "method",
            Self::Param => "param",
            Self::Annotation => "annotation",
        }
    }

    pub fn mask(self) -> u16 {
        1 << self as u8
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Annotation {
    pub name: Type,
    pub value: Option<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub(crate) struct Field {
    pub info: Info,
    pub name: String,
    pub span: Span,
    pub ordinal: Option<u16>,
    pub code_order: u16,
    pub in_union: bool,
    pub kind: FieldKind,
    pub annotations: Vec<Annotation>,
    pub annotation_target: AnnotationTarget,
}

#[derive(Clone, Debug)]
pub(crate) enum FieldKind {
    Slot { ty: Type, default: Option<Expr> },
    Group(usize),
}

#[derive(Clone, Debug)]
pub(crate) struct Enumerant {
    pub info: Info,
    pub name: String,
    pub ordinal: u16,
    pub code_order: u16,
    pub annotations: Vec<Annotation>,
}

#[derive(Clone, Debug)]
pub(crate) enum NodeKind {
    File,
    Struct(Vec<Field>),
    Enum(Vec<Enumerant>),
    Const {
        ty: Type,
        value: Expr,
    },
    Annotation {
        ty: Type,
        targets: u16,
    },
    Interface {
        methods: Vec<Method>,
        superclasses: Vec<Type>,
    },
}

#[derive(Clone, Debug)]
pub(crate) enum ParamList {
    Inline(usize),
    Type(Type),
    Stream(Type),
}

#[derive(Clone, Debug)]
pub(crate) struct Method {
    pub info: Info,
    pub parameters: Vec<String>,
    pub name: String,
    pub span: Span,
    pub ordinal: u16,
    pub code_order: u16,
    pub params: ParamList,
    pub results: ParamList,
    pub annotations: Vec<Annotation>,
}

#[derive(Clone, Debug)]
pub(crate) struct Alias {
    pub name: String,
    pub span: Span,
    pub target: Type,
}

#[derive(Clone, Debug)]
pub(crate) struct Import {
    pub metadata: bool,
    pub path: String,
    pub span: Span,
}

pub(crate) struct Parsed {
    pub nodes: Vec<Node>,
    pub imports: Vec<Import>,
}

#[derive(Clone, Debug)]
pub(crate) struct Node {
    pub info: Info,
    pub parameters: Vec<String>,
    pub name: String,
    pub span: Span,
    pub id: u64,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub aliases: Vec<Alias>,
    pub file: usize,
    pub is_group: bool,
    // Lexically enclosed by the interface, but emitted with scopeId = 0.
    pub method: Option<(u16, bool)>,
    pub kind: NodeKind,
    pub annotations: Vec<Annotation>,
}

fn lex(source: &Source<'_>) -> Result<Vec<Token>, Diagnostic> {
    let bytes = source.text.as_bytes();
    let mut result = Vec::new();
    let mut pos = 0;
    while pos < bytes.len() {
        let start = pos;
        let kind = match bytes[pos] {
            b' ' | b'\n' | b'\r' | b'\t' | 11 | 12 => {
                pos += 1;
                continue;
            }
            0xef if bytes[pos..].starts_with(b"\xef\xbb\xbf") => {
                pos += 3;
                continue;
            }
            b'#' => {
                while pos < bytes.len() && bytes[pos] != b'\n' {
                    pos += 1;
                }
                continue;
            }
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                pos += 1;
                while pos < bytes.len()
                    && (bytes[pos].is_ascii_alphanumeric() || bytes[pos] == b'_')
                {
                    pos += 1;
                }
                TokenKind::Name(source.text[start..pos].into())
            }
            b'0' if bytes[pos..].starts_with(b"0x\"") => {
                pos += 3;
                let mut data = Vec::new();
                loop {
                    // KJ whitespace includes vertical tab, unlike Rust's
                    // u8::is_ascii_whitespace(). Keep binary and token spacing aligned.
                    while pos < bytes.len()
                        && matches!(bytes[pos], b' ' | b'\n' | b'\r' | b'\t' | 11 | 12)
                    {
                        pos += 1;
                    }
                    if pos < bytes.len() && bytes[pos] == b'"' && !data.is_empty() {
                        pos += 1;
                        break;
                    }
                    let pair = bytes.get(pos..pos + 2).and_then(|pair| {
                        let hi = char::from(pair[0]).to_digit(16)?;
                        let lo = char::from(pair[1]).to_digit(16)?;
                        Some(((hi << 4) | lo) as u8)
                    });
                    let Some(byte) = pair else {
                        return Err(source.error(Span { start, end: pos },
                            "binary literals require pairs of hexadecimal digits and a closing quote"));
                    };
                    data.push(byte);
                    pos += 2;
                }
                TokenKind::Data(data)
            }
            b'0'..=b'9' => {
                pos += 1;
                while pos < bytes.len()
                    && (bytes[pos].is_ascii_alphanumeric()
                        || bytes[pos] == b'.'
                        || ((bytes[pos] == b'+' || bytes[pos] == b'-')
                            && matches!(bytes[pos - 1], b'e' | b'E')))
                {
                    pos += 1;
                }
                let text = &source.text[start..pos];
                // C++ tries its octal-integer lexer before the decimal-float
                // lexer. A leading-zero prefix ending at 8 or 9 becomes a
                // separate integer token, so `08e1` and `078.5` are not floats.
                // Digits after a decimal point/exponent (or 0x) are unaffected.
                if text.starts_with('0')
                    && text
                        .bytes()
                        .take_while(u8::is_ascii_digit)
                        .any(|b| b == b'8' || b == b'9')
                {
                    return Err(source.error(
                        Span { start, end: pos },
                        "invalid octal digit in leading-zero number",
                    ));
                }
                // Every loaded file must be lexically valid, including values
                // in declarations that dependency selection never type-checks.
                // Check shape here, not integer range: destination/range checks
                // remain part of lazy value evaluation.
                let valid = if let Some(digits) = text.strip_prefix("0x") {
                    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_hexdigit())
                } else if text.bytes().all(|b| b.is_ascii_digit()) {
                    true
                } else {
                    text.bytes()
                        .all(|b| b.is_ascii_digit() || b".eE+-".contains(&b))
                        && text.parse::<f64>().is_ok()
                };
                if !valid {
                    return Err(source.error(Span { start, end: pos }, "invalid numeric literal"));
                }
                TokenKind::Number(text.into())
            }
            b'`' => {
                pos += 1;
                let content = pos;
                while pos < bytes.len() && !matches!(bytes[pos], b'\r' | b'\n') {
                    pos += 1;
                }
                let mut value = bytes[content..pos].to_vec();
                value.push(b'\n');
                TokenKind::Text(value)
            }
            b'"' => {
                pos += 1;
                let mut value = Vec::new();
                let mut closed = false;
                while pos < bytes.len() {
                    let byte = bytes[pos];
                    pos += 1;
                    match byte {
                        b'"' => {
                            closed = true;
                            break;
                        }
                        b'\\' => {
                            let escaped = *bytes.get(pos).ok_or_else(|| {
                                source.error(Span { start, end: pos }, "unterminated string escape")
                            })?;
                            pos += 1;
                            value.push(match escaped {
                                b'a' => 7,
                                b'b' => 8,
                                b'f' => 12,
                                b'n' => b'\n',
                                b'r' => b'\r',
                                b't' => b'\t',
                                b'v' => 11,
                                b'\\' | b'"' | b'\'' | b'?' => escaped,
                                b'x' => {
                                    let pair = bytes
                                        .get(pos..pos + 2)
                                        .and_then(|pair| {
                                            let hi = char::from(pair[0]).to_digit(16)?;
                                            let lo = char::from(pair[1]).to_digit(16)?;
                                            Some(((hi << 4) | lo) as u8)
                                        })
                                        .ok_or_else(|| {
                                            source.error(Span { start, end: pos },
                                        "hexadecimal string escapes require exactly two digits")
                                        })?;
                                    pos += 2;
                                    pair
                                }
                                b'0'..=b'7' => {
                                    let mut value = escaped - b'0';
                                    for _ in 0..2 {
                                        match bytes.get(pos) {
                                            Some(b'0'..=b'7') => {
                                                value = value.wrapping_mul(8) | (bytes[pos] - b'0');
                                                pos += 1;
                                            }
                                            _ => break,
                                        }
                                    }
                                    value
                                }
                                _ => {
                                    if !escaped.is_ascii() {
                                        pos += source.text[pos - 1..]
                                            .chars()
                                            .next()
                                            .unwrap()
                                            .len_utf8()
                                            - 1;
                                    }
                                    return Err(source.error(
                                        Span { start, end: pos },
                                        "unsupported string escape",
                                    ));
                                }
                            });
                        }
                        b'\n' => {
                            return Err(source
                                .error(Span { start, end: pos }, "unescaped newline in string"))
                        }
                        _ => value.push(byte),
                    }
                }
                if !closed {
                    return Err(
                        source.error(Span { start, end: pos }, "unterminated string literal")
                    );
                }
                TokenKind::Text(value)
            }
            b'@' | b':' | b'=' | b'.' | b'-' | b'$' | b'!' | b'%' | b'&' | b'*' | b'+' | b'/'
            | b'<' | b'>' | b'?' | b'^' | b'|' | b'~' => {
                pos += 1;
                while pos < bytes.len() && b"!$%&*+-./:<=>?@^|~".contains(&bytes[pos]) {
                    pos += 1;
                }
                if &source.text[start..pos] == "->" {
                    TokenKind::Arrow
                } else if pos != start + 1 {
                    return Err(source.error(
                        Span { start, end: pos },
                        format!(
                            "unsupported operator '{}'; adjacent operators require whitespace",
                            &source.text[start..pos]
                        ),
                    ));
                } else {
                    TokenKind::Punct(char::from(bytes[start]))
                }
            }
            b';' | b'{' | b'}' | b'(' | b')' | b',' | b'[' | b']' => {
                pos += 1;
                TokenKind::Punct(char::from(bytes[start]))
            }
            _ => {
                pos += source.text[pos..].chars().next().unwrap().len_utf8();
                return Err(source.error(Span { start, end: pos }, "unexpected character"));
            }
        };
        if result.len() == MAX_TOKENS {
            return Err(source.error(Span { start, end: pos }, "token limit exceeded"));
        }
        result.push(Token {
            kind,
            span: Span { start, end: pos },
        });
    }
    result.push(Token {
        kind: TokenKind::End,
        span: Span {
            start: pos,
            end: pos,
        },
    });
    Ok(result)
}

pub(crate) fn integer(text: &str) -> Option<u64> {
    let (digits, radix) = if let Some(hex) = text.strip_prefix("0x") {
        (hex, 16)
    } else if text.starts_with('0') && text.len() > 1 {
        (&text[1..], 8)
    } else {
        (text, 10)
    };
    if digits.is_empty() || !digits.bytes().all(|b| char::from(b).is_digit(radix)) {
        return None;
    }
    u64::from_str_radix(digits, radix).ok()
}

struct Parser<'a> {
    source_info: std::collections::BTreeMap<usize, Info>,
    source: &'a Source<'a>,
    tokens: Vec<Token>,
    pos: usize,
    nodes: Vec<Node>,
    imports: Vec<Import>,
}

impl Parser<'_> {
    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn error(&self, message: impl Into<String>) -> Diagnostic {
        self.source.error(self.peek().span, message)
    }

    fn take(&mut self) -> Token {
        let token = self.peek().clone();
        if token.kind != TokenKind::End {
            self.pos += 1;
        }
        token
    }

    fn eat(&mut self, ch: char) -> bool {
        if self.peek().kind == TokenKind::Punct(ch) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, ch: char) -> Result<(), Diagnostic> {
        if self.eat(ch) {
            Ok(())
        } else {
            Err(self.error(format!("expected '{ch}'")))
        }
    }

    fn name(&mut self, is_type: bool) -> Result<(String, Span), Diagnostic> {
        let token = self.take();
        let TokenKind::Name(name) = token.kind else {
            return Err(self.source.error(token.span, "expected a declaration name"));
        };
        self.check_name(&name, token.span, is_type)?;
        Ok((name, token.span))
    }

    fn check_name(&self, name: &str, span: Span, is_type: bool) -> Result<(), Diagnostic> {
        if name.contains('_')
            || (is_type && !name.as_bytes()[0].is_ascii_uppercase())
            || (!is_type && !name.as_bytes()[0].is_ascii_lowercase())
        {
            return Err(self.source.error(
                span,
                if is_type {
                    "type names must start with an uppercase letter and contain no underscores"
                } else {
                    "member names must start with a lowercase letter and contain no underscores"
                },
            ));
        }
        Ok(())
    }

    fn number(&mut self) -> Result<(u64, Span), Diagnostic> {
        let token = self.take();
        if let TokenKind::Number(text) = &token.kind {
            if let Some(value) = integer(text) {
                return Ok((value, token.span));
            }
        }
        Err(self
            .source
            .error(token.span, "expected an unsigned 64-bit integer"))
    }

    fn id(&mut self) -> Result<u64, Diagnostic> {
        self.expect('@')?;
        let (id, span) = self.number()?;
        if id >> 63 == 0 {
            return Err(self
                .source
                .error(span, "schema IDs must have the high bit set"));
        }
        Ok(id)
    }

    fn ordinal(&mut self) -> Result<u16, Diagnostic> {
        self.expect('@')?;
        let (value, span) = self.number()?;
        u16::try_from(value).map_err(|_| self.source.error(span, "ordinal exceeds UInt16"))
    }

    fn depth(&self, depth: usize) -> Result<(), Diagnostic> {
        if depth >= MAX_DEPTH {
            Err(self.error("nesting limit exceeded (64)"))
        } else {
            Ok(())
        }
    }

    fn path_string(&mut self, kind: &str) -> Result<(String, Span), Diagnostic> {
        let token = self.take();
        let TokenKind::Text(bytes) = token.kind else {
            return Err(self
                .source
                .error(token.span, format!("expected an {kind} path string")));
        };
        let path = String::from_utf8(bytes).map_err(|_| {
            self.source
                .error(token.span, format!("{kind} paths must be UTF-8"))
        })?;
        if path.is_empty() || path.chars().any(|c| c.is_control() || c == '\\') {
            return Err(self.source.error(
                token.span,
                format!("invalid {kind} path; use nonempty slash-separated paths"),
            ));
        }
        Ok((path, token.span))
    }

    fn ty(&mut self, depth: usize, metadata: bool) -> Result<Type, Diagnostic> {
        let ty = self.type_atom(depth, metadata)?;
        self.type_suffix(ty, depth, metadata)
    }

    fn type_atom(&mut self, depth: usize, metadata: bool) -> Result<Type, Diagnostic> {
        self.depth(depth)?;
        let start = self.peek().span.start;
        let kind = if self.eat('(') {
            let inner = self.ty(depth + 1, metadata)?;
            self.eat(',');
            self.expect(')')?;
            return Ok(inner);
        } else {
            let absolute = self.eat('.');
            let token = self.take();
            let TokenKind::Name(name) = token.kind else {
                return Err(self.source.error(token.span, "expected a type name"));
            };
            if name == "import" && !absolute && matches!(self.peek().kind, TokenKind::Text(_)) {
                let (path_name, path_span) = self.path_string("import")?;
                let index = self
                    .imports
                    .iter()
                    .position(|i| i.path == path_name)
                    .unwrap_or_else(|| {
                        let index = self.imports.len();
                        self.imports.push(Import {
                            path: path_name,
                            span: path_span,
                            metadata,
                        });
                        index
                    });
                self.imports[index].metadata |= metadata;
                TypeKind::Import(index)
            } else {
                TypeKind::Name { name, absolute }
            }
        };
        Ok(Type {
            kind,
            span: Span {
                start,
                end: self.tokens[self.pos - 1].span.end,
            },
        })
    }

    fn annotation_expression(
        &mut self,
        depth: usize,
    ) -> Result<(Type, Option<(Arguments, Span)>), Diagnostic> {
        self.depth(depth)?;
        let (mut name, mut arguments) = if self.eat('(') {
            let expression = self.annotation_expression(depth + 1)?;
            self.eat(',');
            self.expect(')')?;
            expression
        } else {
            (self.type_atom(depth, true)?, None)
        };
        let mut postfix = 0;
        while matches!(self.peek().kind, TokenKind::Punct('.' | '(')) {
            postfix += 1;
            self.depth(depth + postfix)?;
            if let Some((args, span)) = arguments.take() {
                let mut types = Vec::new();
                for (label, value) in args {
                    let ExprKind::Reference(ty) = value.kind else {
                        return Err(self
                            .source
                            .error(value.span, "generic arguments must be types"));
                    };
                    if label.is_some() {
                        return Err(self
                            .source
                            .error(value.span, "generic arguments cannot be named"));
                    }
                    self.type_metadata(&ty);
                    types.push(ty);
                }
                name = Type {
                    kind: TypeKind::Apply(Box::new(name), types),
                    span,
                };
            }
            if self.eat('.') {
                let token = self.take();
                let TokenKind::Name(member) = token.kind else {
                    return Err(self
                        .source
                        .error(token.span, "expected an annotation member name"));
                };
                let span = Span {
                    start: name.span.start,
                    end: token.span.end,
                };
                name = Type {
                    kind: TypeKind::Member(Box::new(name), member),
                    span,
                };
            } else {
                self.expect('(')?;
                let mut args = Vec::new();
                if !self.eat(')') {
                    loop {
                        let label = if matches!(self.peek().kind, TokenKind::Name(_))
                            && self.tokens[self.pos + 1].kind == TokenKind::Punct('=')
                        {
                            let token = self.take();
                            let TokenKind::Name(name) = token.kind else {
                                unreachable!()
                            };
                            self.expect('=')?;
                            Some((name, token.span))
                        } else {
                            None
                        };
                        args.push((label, self.value(depth + postfix)?));
                        if self.eat(')') {
                            break;
                        }
                        self.expect(',')?;
                        if self.eat(')') {
                            break;
                        }
                    }
                }
                arguments = Some((
                    args,
                    Span {
                        start: name.span.start,
                        end: self.tokens[self.pos - 1].span.end,
                    },
                ));
            }
        }
        Ok((name, arguments))
    }

    fn type_metadata(&mut self, ty: &Type) {
        match &ty.kind {
            TypeKind::Import(index) => self.imports[*index].metadata = true,
            TypeKind::Member(parent, _) => self.type_metadata(parent),
            TypeKind::Apply(parent, arguments) => {
                self.type_metadata(parent);
                for argument in arguments {
                    self.type_metadata(argument);
                }
            }
            TypeKind::Name { .. } => (),
        }
    }

    fn annotation(&mut self, depth: usize) -> Result<Annotation, Diagnostic> {
        let start = self.peek().span.start;
        self.expect('$')?;
        let (name, arguments) = self.annotation_expression(depth)?;
        let value = if let Some((mut args, span)) = arguments {
            if args.len() == 1 && args[0].0.is_none() {
                Some(args.pop().unwrap().1)
            } else {
                let mut fields = Vec::new();
                for (name, value) in args {
                    let Some((name, span)) = name else {
                        return Err(self
                            .source
                            .error(value.span, "missing field name in struct value"));
                    };
                    fields.push(Assignment { name, span, value });
                }
                Some(Expr {
                    kind: ExprKind::Struct(fields),
                    span,
                })
            }
        } else {
            None
        };
        Ok(Annotation {
            name,
            value,
            span: Span {
                start,
                end: self.tokens[self.pos - 1].span.end,
            },
        })
    }

    fn annotations(&mut self, depth: usize) -> Result<Vec<Annotation>, Diagnostic> {
        let mut result = Vec::new();
        while self.peek().kind == TokenKind::Punct('$') {
            result.push(self.annotation(depth)?);
        }
        Ok(result)
    }

    fn type_suffix(
        &mut self,
        mut ty: Type,
        depth: usize,
        metadata: bool,
    ) -> Result<Type, Diagnostic> {
        let start = ty.span.start;
        let mut postfix = 0;
        loop {
            if !matches!(self.peek().kind, TokenKind::Punct('.' | '(')) {
                break;
            }
            postfix += 1;
            self.depth(depth + postfix)?;
            let kind = if self.eat('.') {
                let token = self.take();
                let TokenKind::Name(name) = token.kind else {
                    return Err(self.source.error(token.span, "expected a member name"));
                };
                TypeKind::Member(Box::new(ty), name)
            } else {
                self.expect('(')?;
                let mut arguments = Vec::new();
                if !self.eat(')') {
                    loop {
                        arguments.push(self.ty(depth + postfix, metadata)?);
                        if self.eat(')') {
                            break;
                        }
                        self.expect(',')?;
                        if self.eat(')') {
                            break;
                        }
                    }
                }
                TypeKind::Apply(Box::new(ty), arguments)
            };
            ty = Type {
                kind,
                span: Span {
                    start,
                    end: self.tokens[self.pos - 1].span.end,
                },
            };
        }
        Ok(ty)
    }

    fn using(&mut self, parent: usize, depth: usize) -> Result<(), Diagnostic> {
        self.take(); // using
        let explicit = if matches!(self.peek().kind, TokenKind::Name(_))
            && self.tokens[self.pos + 1].kind == TokenKind::Punct('=')
        {
            let token = self.take();
            let TokenKind::Name(name) = token.kind else {
                unreachable!()
            };
            self.expect('=')?;
            Some((name, token.span))
        } else {
            None
        };
        let target = self.ty(depth, true)?;
        let (name, span) = match explicit {
            Some(name) => name,
            None => match &target.kind {
                TypeKind::Member(_, name) => (name.clone(), target.span),
                _ => {
                    return Err(self.source.error(
                        target.span,
                        "using without '=' requires a member from another scope",
                    ))
                }
            },
        };
        self.expect(';')?;
        let target_name = match &target.kind {
            TypeKind::Name { name, .. } | TypeKind::Member(_, name) => Some(name.as_str()),
            _ => None,
        };
        self.check_name(
            &name,
            span,
            target_name.is_none_or(|n| !n.as_bytes()[0].is_ascii_lowercase()),
        )?;
        self.unique_name(parent, &name, span)?;
        self.nodes[parent]
            .aliases
            .push(Alias { name, span, target });
        Ok(())
    }

    fn unique_name(&self, parent: usize, name: &str, span: Span) -> Result<(), Diagnostic> {
        if self.nodes[parent]
            .children
            .iter()
            .any(|&i| self.nodes[i].name == name)
            || self.nodes[parent].aliases.iter().any(|a| a.name == name)
        {
            Err(self
                .source
                .error(span, format!("duplicate declaration '{name}'")))
        } else {
            Ok(())
        }
    }

    fn value(&mut self, depth: usize) -> Result<Expr, Diagnostic> {
        self.depth(depth)?;
        let start = self.peek().span.start;
        if self.eat('[') {
            let mut values = Vec::new();
            if !self.eat(']') {
                loop {
                    values.push(self.value(depth + 1)?);
                    if self.eat(']') {
                        break;
                    }
                    self.expect(',')?;
                    if self.eat(']') {
                        break;
                    }
                }
            }
            return Ok(Expr {
                kind: ExprKind::List(values),
                span: Span {
                    start,
                    end: self.tokens[self.pos - 1].span.end,
                },
            });
        }
        if self.eat('(') {
            let mut entries = Vec::new();
            if !self.eat(')') {
                loop {
                    let name = if matches!(self.peek().kind, TokenKind::Name(_))
                        && self.tokens[self.pos + 1].kind == TokenKind::Punct('=')
                    {
                        let token = self.take();
                        let TokenKind::Name(name) = token.kind else {
                            unreachable!()
                        };
                        self.expect('=')?;
                        Some((name, token.span))
                    } else {
                        None
                    };
                    entries.push((name, self.value(depth + 1)?));
                    if self.eat(')') {
                        break;
                    }
                    self.expect(',')?;
                    if self.eat(')') {
                        break;
                    }
                }
            }
            if entries.len() == 1 && entries[0].0.is_none() {
                let mut value = entries.pop().unwrap().1;
                if matches!(self.peek().kind, TokenKind::Punct('.' | '(')) {
                    let ExprKind::Reference(reference) = value.kind else {
                        return Err(self.error("literal values have no members or type arguments"));
                    };
                    let reference = self.type_suffix(reference, depth + 1, false)?;
                    value = Expr {
                        span: reference.span,
                        kind: ExprKind::Reference(reference),
                    };
                }
                return Ok(value);
            }
            let mut assignments = Vec::new();
            for (name, value) in entries {
                let Some((name, span)) = name else {
                    return Err(self
                        .source
                        .error(value.span, "missing field name in struct value"));
                };
                assignments.push(Assignment { name, span, value });
            }
            return Ok(Expr {
                kind: ExprKind::Struct(assignments),
                span: Span {
                    start,
                    end: self.tokens[self.pos - 1].span.end,
                },
            });
        }
        if self.peek().kind == TokenKind::Name("embed".into())
            && matches!(self.tokens[self.pos + 1].kind, TokenKind::Text(_))
        {
            self.take();
            let (path, span) = self.path_string("embed")?;
            return Ok(Expr {
                kind: ExprKind::Embed(path),
                span: Span {
                    start,
                    end: span.end,
                },
            });
        }
        if matches!(self.peek().kind, TokenKind::Name(_) | TokenKind::Punct('.')) {
            let reference = self.ty(depth, false)?;
            return Ok(Expr {
                span: reference.span,
                kind: ExprKind::Reference(reference),
            });
        }
        let negative = self.eat('-');
        let mut token = self.take();
        if negative {
            match &mut token.kind {
                TokenKind::Number(value) => value.insert(0, '-'),
                TokenKind::Name(value) if value == "inf" => value.insert(0, '-'),
                _ => {
                    return Err(self
                        .source
                        .error(token.span, "expected a numeric value after '-'"))
                }
            }
            token.span.start = start;
        }
        if let TokenKind::Text(text) = &mut token.kind {
            while let TokenKind::Text(part) = &self.peek().kind {
                text.extend_from_slice(part);
                token.span.end = self.take().span.end;
            }
        }
        if !matches!(
            token.kind,
            TokenKind::Number(_) | TokenKind::Text(_) | TokenKind::Data(_)
        ) && !matches!(&token.kind, TokenKind::Name(n) if n == "-inf")
        {
            return Err(self.source.error(
                token.span,
                "expected a literal, list, struct value or constant reference",
            ));
        }
        Ok(Expr {
            kind: ExprKind::Literal(token.kind),
            span: token.span,
        })
    }

    fn default(&mut self, depth: usize) -> Result<Option<Expr>, Diagnostic> {
        if self.eat('=') {
            self.value(depth).map(Some)
        } else {
            Ok(None)
        }
    }

    fn new_node(
        &mut self,
        parent: usize,
        name: String,
        span: Span,
        id: u64,
        is_group: bool,
    ) -> Result<usize, Diagnostic> {
        if self.nodes.len() == MAX_NODES {
            return Err(self.error("node limit exceeded (4096)"));
        }
        let index = self.nodes.len();
        if !is_group {
            self.nodes[parent].children.push(index);
        }
        self.nodes.push(Node {
            info: Info::default(),
            name,
            span,
            id,
            parent: Some(parent),
            children: vec![],
            aliases: vec![],
            file: 0,
            is_group,
            method: None,
            parameters: vec![],
            kind: NodeKind::File,
            annotations: vec![],
        });
        Ok(index)
    }

    fn struct_body(
        &mut self,
        index: usize,
        depth: usize,
        in_union: bool,
        fields: &mut Vec<Field>,
        names: &mut BTreeSet<String>,
    ) -> Result<(), Diagnostic> {
        self.depth(depth)?;
        let start = fields.len();
        while !self.eat('}') {
            if matches!(&self.peek().kind, TokenKind::Name(n) if n == "union")
                && self.tokens[self.pos + 1].kind == TokenKind::Punct('{')
            {
                if in_union {
                    return Err(self.error("unions cannot contain unnamed unions"));
                }
                if !names.insert(String::new()) {
                    return Err(self.error("an unnamed union is already defined in this scope"));
                }
                self.take();
                self.expect('{')?;
                self.struct_body(index, depth + 1, true, fields, names)?;
                continue;
            }
            if matches!(&self.peek().kind, TokenKind::Name(n) if n == "using" || n == "struct" || n == "enum" || n == "const" || n == "annotation" || n == "interface")
                && !matches!(self.tokens[self.pos + 1].kind, TokenKind::Punct('@' | ':'))
            {
                if in_union || self.nodes[index].is_group {
                    return Err(self
                        .error("type declarations and aliases do not belong in groups or unions"));
                }
                if matches!(&self.peek().kind, TokenKind::Name(n) if n == "using") {
                    self.using(index, depth + 1)?;
                } else {
                    self.declaration(index, depth + 1)?;
                }
                continue;
            }
            let (name, span) = self.name(false)?;
            if !names.insert(name.clone()) {
                return Err(self
                    .source
                    .error(span, format!("duplicate member '{name}'")));
            }
            if fields.len() == u16::MAX as usize {
                return Err(self.error("member limit exceeded (65535)"));
            }
            let ordinal = if self.peek().kind == TokenKind::Punct('@') {
                Some(self.ordinal()?)
            } else {
                None
            };
            let legacy_union = self.eat('!');
            self.expect(':')?;
            let annotations;
            let annotation_target;
            // An ordinal selects a field, even when its type parameter is
            // named `group`. Named unions also admit legacy explicit ordinals.
            let kind = if matches!(&self.peek().kind, TokenKind::Name(n) if (n == "group" && ordinal.is_none()) || n == "union")
            {
                let is_union = matches!(&self.take().kind, TokenKind::Name(n) if n == "union");
                if ordinal.is_some() && (!is_union || !legacy_union) {
                    return Err(self.source.error(
                        span,
                        "groups have no ordinal; legacy union ordinals require @n!",
                    ));
                }
                if legacy_union && ordinal.is_none() {
                    return Err(self
                        .source
                        .error(span, "'!' requires a legacy union ordinal"));
                }
                annotations = self.annotations(depth + 1)?;
                annotation_target = if is_union {
                    AnnotationTarget::Union
                } else {
                    AnnotationTarget::Group
                };
                self.expect('{')?;
                let group = self.new_node(index, name.clone(), span, 0, true)?;
                let mut members = Vec::new();
                self.struct_body(
                    group,
                    depth + 1,
                    is_union,
                    &mut members,
                    &mut BTreeSet::new(),
                )?;
                if members.is_empty() {
                    return Err(self
                        .source
                        .error(span, "group must have at least one member"));
                }
                self.nodes[group].info = self
                    .source_info
                    .get(&span.start)
                    .cloned()
                    .unwrap_or_default();
                self.nodes[group].kind = NodeKind::Struct(members);
                FieldKind::Group(group)
            } else {
                if ordinal.is_none() || legacy_union {
                    return Err(self.source.error(span, "fields require an ordinal (@n)"));
                }
                let ty = self.ty(depth + 1, true)?;
                let default = self.default(depth + 1)?;
                annotations = self.annotations(depth + 1)?;
                annotation_target = AnnotationTarget::Field;
                self.expect(';')?;
                FieldKind::Slot { ty, default }
            };
            fields.push(Field {
                info: self
                    .source_info
                    .get(&span.start)
                    .cloned()
                    .unwrap_or_default(),
                name,
                span,
                ordinal,
                code_order: fields.len() as u16,
                in_union,
                kind,
                annotations,
                annotation_target,
            });
        }
        for field in fields.iter() {
            self.unique_name(index, &field.name, field.span)?;
        }
        if in_union && fields.len() - start < 2 {
            return Err(self.source.error(
                self.nodes[index].span,
                "union must have at least two members",
            ));
        }
        Ok(())
    }

    // Groups share the containing struct's ordinal namespace. Their field-list
    // position (and hence ID) is determined by the first explicit ordinal below
    // them, including an optional legacy named-union ordinal.
    fn order_fields(&mut self, index: usize, ordinals: &mut Vec<(u16, Span)>) -> u16 {
        let NodeKind::Struct(mut fields) =
            std::mem::replace(&mut self.nodes[index].kind, NodeKind::File)
        else {
            unreachable!()
        };
        let mut keyed = Vec::with_capacity(fields.len());
        for field in fields.drain(..) {
            if let Some(ordinal) = field.ordinal {
                ordinals.push((ordinal, field.span));
            }
            let child = match field.kind {
                FieldKind::Group(group) => self.order_fields(group, ordinals),
                FieldKind::Slot { .. } => u16::MAX,
            };
            let first = child.min(field.ordinal.unwrap_or(u16::MAX));
            keyed.push((first, field));
        }
        keyed.sort_by_key(|(key, _)| *key);
        let first = keyed.first().map_or(u16::MAX, |(key, _)| *key);
        self.nodes[index].kind = NodeKind::Struct(keyed.into_iter().map(|(_, f)| f).collect());
        first
    }

    fn check_ordinals(&self, ordinals: &mut [(u16, Span)]) -> Result<(), Diagnostic> {
        ordinals.sort_by_key(|&(ordinal, _)| ordinal);
        if ordinals.len() > u16::MAX as usize {
            return Err(self.error("member limit exceeded (65535)"));
        }
        for (expected, &(ordinal, span)) in ordinals.iter().enumerate() {
            if ordinal as usize != expected {
                return Err(self.source.error(span, format!("ordinals must be unique and contiguous from zero; expected @{expected}, found @{ordinal}")));
            }
        }
        Ok(())
    }

    fn declaration(&mut self, parent: usize, depth: usize) -> Result<(), Diagnostic> {
        self.depth(depth)?;
        let keyword = self.take();
        if matches!(&keyword.kind, TokenKind::Name(n) if n == "const" || n == "annotation") {
            let is_annotation = matches!(&keyword.kind, TokenKind::Name(n) if n == "annotation");
            let (name, span) = self.name(false)?;
            self.unique_name(parent, &name, span)?;
            let id = if self.peek().kind == TokenKind::Punct('@') {
                self.id()?
            } else {
                0
            };
            let mut targets = 0u16;
            if is_annotation {
                self.expect('(')?;
                if !self.eat(')') {
                    loop {
                        let token = self.take();
                        let mask = match &token.kind {
                            TokenKind::Punct('*') => (1 << AnnotationTarget::ALL.len()) - 1,
                            TokenKind::Name(name) => AnnotationTarget::ALL
                                .iter()
                                .find(|t| t.name() == name)
                                .map(|t| t.mask())
                                .ok_or_else(|| {
                                    self.source.error(token.span, "invalid annotation target")
                                })?,
                            _ => {
                                return Err(self
                                    .source
                                    .error(token.span, "expected an annotation target"))
                            }
                        };
                        if targets & mask != 0 {
                            return Err(self.source.error(
                                token.span,
                                "duplicate annotation target or mixed wildcard",
                            ));
                        }
                        targets |= mask;
                        if self.eat(')') {
                            break;
                        }
                        self.expect(',')?;
                        if self.eat(')') {
                            break;
                        }
                    }
                }
            }
            self.expect(':')?;
            // C++ omits imports used only in an annotation's type from the
            // requested-file import table; its schema dependencies still load.
            let ty = self.ty(depth + 1, !is_annotation)?;
            let kind = if is_annotation {
                NodeKind::Annotation { ty, targets }
            } else {
                self.expect('=')?;
                NodeKind::Const {
                    ty,
                    value: self.value(depth + 1)?,
                }
            };
            let annotations = self.annotations(depth + 1)?;
            self.expect(';')?;
            let index = self.new_node(parent, name, span, id, false)?;
            self.nodes[index].info = self
                .source_info
                .get(&keyword.span.start)
                .cloned()
                .unwrap_or_default();
            self.nodes[index].kind = kind;
            self.nodes[index].annotations = annotations;
            return Ok(());
        }
        let declaration_kind = match &keyword.kind {
            TokenKind::Name(name) if matches!(name.as_str(), "struct" | "enum" | "interface") => {
                name.as_str()
            }
            _ => {
                return Err(self.source.error(
                    keyword.span,
                    "expected struct, enum, interface, const, annotation or using",
                ))
            }
        };
        let (name, span) = self.name(true)?;
        self.unique_name(parent, &name, span)?;
        let id = if self.peek().kind == TokenKind::Punct('@') {
            self.id()?
        } else {
            0
        };
        let parameters = if declaration_kind != "enum" && self.eat('(') {
            self.parameters(')')?
        } else {
            vec![]
        };
        let mut superclasses = Vec::new();
        if declaration_kind == "interface"
            && matches!(&self.peek().kind, TokenKind::Name(n) if n == "extends")
        {
            self.take();
            self.expect('(')?;
            if !self.eat(')') {
                loop {
                    superclasses.push(self.ty(depth + 1, true)?);
                    if self.eat(')') {
                        break;
                    }
                    self.expect(',')?;
                    if self.eat(')') {
                        break;
                    }
                }
            }
        }
        let annotations = self.annotations(depth + 1)?;
        self.expect('{')?;
        let index = self.new_node(parent, name, span, id, false)?;
        self.nodes[index].info = self
            .source_info
            .get(&keyword.span.start)
            .cloned()
            .unwrap_or_default();
        self.nodes[index].annotations = annotations;
        self.nodes[index].parameters = parameters;
        let mut ordinals = Vec::new();
        if declaration_kind == "struct" {
            let mut fields = Vec::new();
            self.struct_body(index, depth, false, &mut fields, &mut BTreeSet::new())?;
            self.nodes[index].kind = NodeKind::Struct(fields);
            self.order_fields(index, &mut ordinals);
        } else if declaration_kind == "interface" {
            let methods = self.interface_body(index, depth + 1)?;
            ordinals.extend(methods.iter().map(|m| (m.ordinal, m.span)));
            self.nodes[index].kind = NodeKind::Interface {
                methods,
                superclasses,
            };
        } else {
            let mut enumerants = Vec::new();
            let mut names = BTreeSet::new();
            while !self.eat('}') {
                let (name, span) = self.name(false)?;
                if !names.insert(name.clone()) {
                    return Err(self
                        .source
                        .error(span, format!("duplicate member '{name}'")));
                }
                if enumerants.len() == u16::MAX as usize {
                    return Err(self.error("member limit exceeded (65535)"));
                }
                let ordinal = self.ordinal()?;
                let annotations = self.annotations(depth + 1)?;
                enumerants.push(Enumerant {
                    info: self
                        .source_info
                        .get(&span.start)
                        .cloned()
                        .unwrap_or_default(),
                    name,
                    ordinal,
                    code_order: enumerants.len() as u16,
                    annotations,
                });
                ordinals.push((ordinal, span));
                self.expect(';')?;
            }
            enumerants.sort_by_key(|e| e.ordinal);
            self.nodes[index].kind = NodeKind::Enum(enumerants);
        }
        self.check_ordinals(&mut ordinals)
    }

    fn parameters(&mut self, close: char) -> Result<Vec<String>, Diagnostic> {
        let mut parameters = Vec::new();
        if !self.eat(close) {
            loop {
                let token = self.take();
                let TokenKind::Name(name) = token.kind else {
                    return Err(self
                        .source
                        .error(token.span, "expected a generic parameter name"));
                };
                if parameters.len() == u16::MAX as usize {
                    return Err(self.error("generic parameter limit exceeded (65535)"));
                }
                parameters.push(name);
                if self.eat(close) {
                    break;
                }
                self.expect(',')?;
                if self.eat(close) {
                    break;
                }
            }
        }
        Ok(parameters)
    }

    fn interface_body(&mut self, parent: usize, depth: usize) -> Result<Vec<Method>, Diagnostic> {
        self.depth(depth)?;
        let mut methods = Vec::new();
        let mut names = BTreeSet::new();
        while !self.eat('}') {
            if matches!(&self.peek().kind, TokenKind::Name(n) if ["struct", "enum", "interface", "const", "annotation", "using"].contains(&n.as_str()))
                && !matches!(self.tokens[self.pos + 1].kind, TokenKind::Punct('@' | ':'))
            {
                if matches!(&self.peek().kind, TokenKind::Name(n) if n == "using") {
                    self.using(parent, depth)?;
                } else {
                    self.declaration(parent, depth)?;
                }
                continue;
            }
            let (name, span) = self.name(false)?;
            if !names.insert(name.clone()) {
                return Err(self
                    .source
                    .error(span, format!("duplicate method '{name}'")));
            }
            if methods.len() == u16::MAX as usize {
                return Err(self.error("method limit exceeded (65535)"));
            }
            let ordinal = self.ordinal()?;
            let parameters = if self.eat('[') {
                self.parameters(']')?
            } else {
                vec![]
            };
            let params = self.param_list(parent, &name, ordinal, false, depth + 1)?;
            let results = if self.peek().kind == TokenKind::Arrow {
                self.take();
                self.param_list(parent, &name, ordinal, true, depth + 1)?
            } else {
                let results = self.method_struct(parent, &name, ordinal, true, span, vec![])?;
                if let ParamList::Inline(index) = results {
                    self.nodes[index].info.span = Span::default();
                }
                results
            };
            for params in [&params, &results] {
                if let ParamList::Inline(index) = params {
                    self.nodes[*index].parameters = parameters.clone();
                }
            }
            let annotations = self.annotations(depth + 1)?;
            self.expect(';')?;
            methods.push(Method {
                info: self
                    .source_info
                    .get(&span.start)
                    .cloned()
                    .unwrap_or_default(),
                parameters,
                name,
                span,
                ordinal,
                code_order: methods.len() as u16,
                params,
                results,
                annotations,
            });
        }
        for method in &methods {
            self.unique_name(parent, &method.name, method.span)?;
        }
        methods.sort_by_key(|m| m.ordinal);
        Ok(methods)
    }

    fn param_list(
        &mut self,
        parent: usize,
        name: &str,
        ordinal: u16,
        results: bool,
        depth: usize,
    ) -> Result<ParamList, Diagnostic> {
        self.depth(depth)?;
        let span = self.peek().span;
        if self.eat('(') {
            let mut fields = Vec::new();
            let mut names = BTreeSet::new();
            if !self.eat(')') {
                loop {
                    let token = self.take();
                    let span = token.span;
                    let TokenKind::Name(name) = token.kind else {
                        return Err(self.source.error(span, "expected a parameter name"));
                    };
                    if !names.insert(name.clone()) {
                        return Err(self
                            .source
                            .error(span, format!("duplicate parameter '{name}'")));
                    }
                    if fields.len() == u16::MAX as usize {
                        return Err(self.error("parameter limit exceeded (65535)"));
                    }
                    self.expect(':')?;
                    let ty = self.ty(depth + 1, true)?;
                    let default = self.default(depth + 1)?;
                    let annotations = self.annotations(depth + 1)?;
                    let position = fields.len() as u16;
                    fields.push(Field {
                        info: Info {
                            span: Span {
                                start: span.start,
                                end: self.tokens[self.pos - 1].span.end,
                            },
                            comment: None,
                        },
                        name,
                        span,
                        ordinal: Some(position),
                        code_order: position,
                        in_union: false,
                        kind: FieldKind::Slot { ty, default },
                        annotations,
                        annotation_target: AnnotationTarget::Param,
                    });
                    if self.eat(')') {
                        break;
                    }
                    self.expect(',')?;
                    if self.eat(')') {
                        break;
                    }
                }
            }
            let params = self.method_struct(parent, name, ordinal, results, span, fields)?;
            if let ParamList::Inline(index) = params {
                self.nodes[index].info.span.end = self.tokens[self.pos - 1].span.end;
            }
            Ok(params)
        } else if matches!(&self.peek().kind, TokenKind::Name(n) if n == "stream") {
            self.take();
            if !results {
                return Err(self.source.error(span, "stream can only appear after '->'"));
            }
            let path = "/capnp/stream.capnp";
            let index = self
                .imports
                .iter()
                .position(|i| i.path == path)
                .unwrap_or_else(|| {
                    let index = self.imports.len();
                    self.imports.push(Import {
                        metadata: true,
                        path: path.into(),
                        span,
                    });
                    index
                });
            self.imports[index].metadata = true;
            Ok(ParamList::Stream(Type {
                kind: TypeKind::Member(
                    Box::new(Type {
                        kind: TypeKind::Import(index),
                        span,
                    }),
                    "StreamResult".into(),
                ),
                span,
            }))
        } else {
            Ok(ParamList::Type(self.ty(depth, true)?))
        }
    }

    fn method_struct(
        &mut self,
        parent: usize,
        name: &str,
        ordinal: u16,
        results: bool,
        span: Span,
        fields: Vec<Field>,
    ) -> Result<ParamList, Diagnostic> {
        let name = format!("{name}${}", if results { "Results" } else { "Params" });
        let index = self.new_node(parent, name, span, 0, false)?;
        self.nodes[parent].children.pop(); // Auxiliary nodes are not nested declarations.
        self.nodes[index].info.span = span;
        self.nodes[index].method = Some((ordinal, results));
        self.nodes[index].kind = NodeKind::Struct(fields);
        Ok(ParamList::Inline(index))
    }
}

pub(crate) fn parse_literal(source: &Source<'_>) -> Result<crate::literal::Literal, Diagnostic> {
    use crate::literal::Literal;
    fn convert(source: &Source<'_>, expr: Expr) -> Result<Literal, Diagnostic> {
        Ok(match expr.kind {
            ExprKind::Literal(TokenKind::Number(s)) => Literal::Number(s),
            ExprKind::Literal(TokenKind::Name(s)) => Literal::Name(s),
            ExprKind::Literal(TokenKind::Text(s)) => Literal::Text(s),
            ExprKind::Literal(TokenKind::Data(s)) => Literal::Data(s),
            ExprKind::Reference(Type {
                kind:
                    TypeKind::Name {
                        name,
                        absolute: false,
                    },
                ..
            }) => Literal::Name(name),
            ExprKind::List(values) => Literal::List(
                values
                    .into_iter()
                    .map(|v| convert(source, v))
                    .collect::<Result<_, _>>()?,
            ),
            ExprKind::Struct(fields) => Literal::Struct(
                fields
                    .into_iter()
                    .map(|f| Ok((f.name, convert(source, f.value)?)))
                    .collect::<Result<_, Diagnostic>>()?,
            ),
            _ => {
                return Err(source.error(
                    expr.span,
                    "external references and embeds are not allowed in text values",
                ))
            }
        })
    }
    let mut parser = Parser {
        source_info: Default::default(),
        source,
        tokens: lex(source)?,
        pos: 0,
        nodes: vec![],
        imports: vec![],
    };
    let value = parser.value(0)?;
    if parser.peek().kind != TokenKind::End {
        return Err(parser.error("extra tokens in text value"));
    }
    convert(source, value)
}

pub(crate) fn parse(source: &Source<'_>, allow_missing_ids: bool) -> Result<Parsed, Diagnostic> {
    parse_with_file_id(source, allow_missing_ids, || {
        getrandom::u64().map_err(|error| error.to_string())
    })
}

fn parse_with_file_id(
    source: &Source<'_>,
    allow_missing_ids: bool,
    random: impl FnOnce() -> Result<u64, String>,
) -> Result<Parsed, Diagnostic> {
    let tokens = lex(source)?;
    let source_info = source_info::index(source.text, &tokens);
    let mut parser = Parser {
        source_info,
        source,
        tokens,
        pos: 0,
        imports: vec![],
        nodes: vec![Node {
            info: Info::default(),
            name: source.filename.into(),
            span: Span::default(),
            id: 0,
            parent: None,
            children: vec![],
            aliases: vec![],
            file: 0,
            is_group: false,
            method: None,
            parameters: vec![],
            kind: NodeKind::File,
            annotations: vec![],
        }],
    };
    while parser.peek().kind != TokenKind::End {
        if parser.peek().kind == TokenKind::Punct('@') {
            if parser.nodes[0].id != 0 {
                return Err(parser.error("duplicate file ID"));
            }
            parser.nodes[0].span = parser.peek().span;
            parser.nodes[0].info.comment = parser
                .source_info
                .get(&parser.peek().span.start)
                .and_then(|i| i.comment.clone());
            parser.nodes[0].id = parser.id()?;
            parser.expect(';')?;
        } else if parser.peek().kind == TokenKind::Punct('$') {
            let annotation = parser.annotation(0)?;
            parser.expect(';')?;
            parser.nodes[0].annotations.push(annotation);
        } else if matches!(&parser.peek().kind, TokenKind::Name(n) if n == "using") {
            parser.using(0, 0)?;
        } else {
            parser.declaration(0, 0)?;
        }
    }
    if parser.nodes[0].id == 0 {
        if !allow_missing_ids {
            return Err(parser.error("missing file ID"));
        }
        // Draw only after the full file has parsed, so explicit IDs may still
        // appear after declarations. Descendants retain the standard ID algorithm.
        parser.nodes[0].id = random()
            .map_err(|error| parser.error(format!("cannot generate file ID: {error}")))?
            | (1 << 63);
    }
    let mut ids = BTreeSet::new();
    for i in 0..parser.nodes.len() {
        if parser.nodes[i].id == 0 {
            let mut digest = md5::Context::new();
            digest.consume(
                parser.nodes[parser.nodes[i].parent.unwrap()]
                    .id
                    .to_le_bytes(),
            );
            if let Some((ordinal, results)) = parser.nodes[i].method {
                digest.consume(ordinal.to_le_bytes());
                digest.consume([u8::from(results)]);
            } else if parser.nodes[i].is_group {
                let NodeKind::Struct(fields) = &parser.nodes[parser.nodes[i].parent.unwrap()].kind
                else {
                    unreachable!()
                };
                let position = fields
                    .iter()
                    .position(|f| matches!(f.kind, FieldKind::Group(g) if g == i))
                    .unwrap();
                digest.consume((position as u16).to_le_bytes());
            } else {
                digest.consume(parser.nodes[i].name.as_bytes());
            }
            let digest = digest.finalize();
            parser.nodes[i].id = u64::from_be_bytes(digest.0[..8].try_into().unwrap()) | (1 << 63);
        }
        if !ids.insert(parser.nodes[i].id) {
            return Err(source.error(
                parser.nodes[i].span,
                format!("duplicate schema ID 0x{:016x}", parser.nodes[i].id),
            ));
        }
    }
    Ok(Parsed {
        nodes: parser.nodes,
        imports: parser.imports,
    })
}
