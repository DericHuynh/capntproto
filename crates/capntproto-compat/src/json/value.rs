//! Ordered JSON values, including the pinned C++ codec's call and raw extensions.
use crate::invalid;
use capnp::Result;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
    Call {
        function: String,
        params: Vec<Value>,
    },
    /// Trusted, already serialized JSON. Never produced by the parser.
    Raw(String),
}
impl Value {
    pub fn get(&self, name: &str) -> Option<&Self> {
        if let Self::Object(fields) = self {
            fields.iter().rev().find(|(k, _)| k == name).map(|(_, v)| v)
        } else {
            None
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        if let Self::String(s) = self {
            Some(s)
        } else {
            None
        }
    }
}

pub(super) fn parse(input: &str, max_depth: usize, max_bytes: usize) -> Result<Value> {
    if input.len() > max_bytes {
        return Err(invalid("JSON input size limit exceeded"));
    }
    struct Parser<'a> {
        input: &'a str,
        pos: usize,
        max_depth: usize,
    }
    impl Parser<'_> {
        fn ws(&mut self) {
            while matches!(
                self.input.as_bytes().get(self.pos),
                Some(b' ' | b'\t' | b'\r' | b'\n')
            ) {
                self.pos += 1;
            }
        }
        fn eat(&mut self, ch: u8) -> bool {
            self.ws();
            if self.input.as_bytes().get(self.pos) == Some(&ch) {
                self.pos += 1;
                true
            } else {
                false
            }
        }
        fn string(&mut self) -> Result<String> {
            self.ws();
            let start = self.pos;
            if !self.eat(b'"') {
                return Err(invalid("expected JSON string"));
            }
            let mut escaped = false;
            while let Some(&b) = self.input.as_bytes().get(self.pos) {
                self.pos += 1;
                if !escaped && b == b'"' {
                    return serde_json::from_str(&self.input[start..self.pos])
                        .map_err(|e| invalid(e.to_string()));
                }
                escaped = !escaped && b == b'\\';
            }
            Err(invalid("unterminated JSON string"))
        }
        fn list(&mut self, end: u8, depth: usize) -> Result<Vec<Value>> {
            let mut values = vec![];
            if !self.eat(end) {
                loop {
                    values.push(self.value(depth)?);
                    if self.eat(end) {
                        break;
                    }
                    if !self.eat(b',') {
                        return Err(invalid("expected JSON comma"));
                    }
                }
            }
            Ok(values)
        }
        fn value(&mut self, depth: usize) -> Result<Value> {
            if depth > self.max_depth {
                return Err(invalid("JSON nesting limit exceeded"));
            }
            self.ws();
            let start = self.pos;
            match self.input.as_bytes().get(self.pos).copied() {
                Some(b'"') => self.string().map(Value::String),
                Some(b'[') => {
                    self.pos += 1;
                    self.list(b']', depth + 1).map(Value::Array)
                }
                Some(b'{') => {
                    self.pos += 1;
                    let mut fields = vec![];
                    if !self.eat(b'}') {
                        loop {
                            let key = self.string()?;
                            if !self.eat(b':') {
                                return Err(invalid("expected JSON colon"));
                            }
                            fields.push((key, self.value(depth + 1)?));
                            if self.eat(b'}') {
                                break;
                            }
                            if !self.eat(b',') {
                                return Err(invalid("expected JSON comma"));
                            }
                        }
                    }
                    Ok(Value::Object(fields))
                }
                Some(b'-' | b'0'..=b'9') => {
                    if self.input.as_bytes()[self.pos] == b'-' {
                        self.pos += 1;
                    }
                    if self.eat_digit(b'0') {
                    } else {
                        if !matches!(self.input.as_bytes().get(self.pos), Some(b'1'..=b'9')) {
                            return Err(invalid("invalid JSON number"));
                        }
                        self.digits();
                    }
                    if self.eat_digit(b'.') {
                        let p = self.pos;
                        self.digits();
                        if p == self.pos {
                            return Err(invalid("invalid fraction"));
                        }
                    }
                    if matches!(self.input.as_bytes().get(self.pos), Some(b'e' | b'E')) {
                        self.pos += 1;
                        if matches!(self.input.as_bytes().get(self.pos), Some(b'+' | b'-')) {
                            self.pos += 1;
                        }
                        let p = self.pos;
                        self.digits();
                        if p == self.pos {
                            return Err(invalid("invalid exponent"));
                        }
                    }
                    self.input[start..self.pos]
                        .parse()
                        .map(Value::Number)
                        .map_err(|_| invalid("invalid JSON number"))
                }
                Some(b'a'..=b'z' | b'A'..=b'Z' | b'_') => {
                    while matches!(
                        self.input.as_bytes().get(self.pos),
                        Some(b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_')
                    ) {
                        self.pos += 1;
                    }
                    let name = &self.input[start..self.pos];
                    match name {
                        "null" => Ok(Value::Null),
                        "true" => Ok(Value::Bool(true)),
                        "false" => Ok(Value::Bool(false)),
                        _ => Err(invalid("unknown JSON identifier")),
                    }
                }
                _ => Err(invalid(format!("invalid JSON at byte {}", self.pos))),
            }
        }
        fn digits(&mut self) {
            while matches!(self.input.as_bytes().get(self.pos), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        fn eat_digit(&mut self, b: u8) -> bool {
            if self.input.as_bytes().get(self.pos) == Some(&b) {
                self.pos += 1;
                true
            } else {
                false
            }
        }
    }
    let mut parser = Parser {
        input,
        pos: 0,
        max_depth,
    };
    let value = parser.value(0)?;
    parser.ws();
    if parser.pos != input.len() {
        return Err(invalid("trailing JSON input"));
    }
    Ok(value)
}

pub(super) fn render(
    value: &Value,
    pretty: bool,
    depth_limit: usize,
    max_bytes: usize,
) -> Result<String> {
    struct Render {
        pretty: bool,
        depth_limit: usize,
        max_bytes: usize,
    }
    impl Render {
        fn value(
            &self,
            v: &Value,
            indent: usize,
            depth: usize,
            has_prefix: bool,
        ) -> Result<(String, bool)> {
            if depth > self.depth_limit {
                return Err(invalid("JSON nesting limit exceeded"));
            }
            let mut multiline = false;
            let output = match v {
                Value::Null => "null".into(),
                Value::Bool(v) => v.to_string(),
                Value::Number(v) if v.is_finite() => crate::format::float64(*v),
                Value::Number(_) => return Err(invalid("non-finite raw JSON number")),
                Value::String(v) => serde_json::to_string(v).map_err(|e| invalid(e.to_string()))?,
                Value::Raw(v) => v.clone(),
                Value::Array(values) | Value::Call { params: values, .. } => {
                    let sub = indent + usize::from(values.len() > 1);
                    let mut children = vec![];
                    let mut child_multiline = false;
                    for v in values {
                        let (s, m) = self.value(v, sub, depth + 1, false)?;
                        child_multiline |= m;
                        children.push(s);
                    }
                    let (prefix, start, end) = if let Value::Call { function, .. } = v {
                        if function.is_empty()
                            || !function.bytes().enumerate().all(|(i, b)| {
                                b.is_ascii_alphabetic()
                                    || b == b'_'
                                    || (i > 0 && b.is_ascii_digit())
                            })
                        {
                            return Err(invalid("invalid JSON call name"));
                        }
                        (function.as_str(), '(', ')')
                    } else {
                        ("", '[', ']')
                    };
                    let (s, m) = self.list(
                        children,
                        child_multiline,
                        indent,
                        has_prefix || matches!(v, Value::Call { .. }),
                    );
                    multiline = m;
                    format!("{prefix}{start}{s}{end}")
                }
                Value::Object(fields) => {
                    let sub = indent + usize::from(fields.len() > 1);
                    let mut children = vec![];
                    let mut child_multiline = false;
                    for (name, value) in fields {
                        let (s, m) = self.value(value, sub, depth + 1, true)?;
                        child_multiline |= m;
                        children.push(format!(
                            "{}:{}{}",
                            serde_json::to_string(name).map_err(|e| invalid(e.to_string()))?,
                            if self.pretty { " " } else { "" },
                            s
                        ));
                    }
                    let (s, m) = self.list(children, child_multiline, indent, has_prefix);
                    multiline = m;
                    format!("{{{s}}}")
                }
            };
            if output.len() > self.max_bytes {
                return Err(invalid("JSON output size limit exceeded"));
            }
            Ok((output, multiline))
        }
        fn list(
            &self,
            items: Vec<String>,
            child_multiline: bool,
            indent: usize,
            has_prefix: bool,
        ) -> (String, bool) {
            if !self.pretty {
                return (items.join(","), false);
            }
            if items.len() > 1 && (child_multiline || items.iter().any(|s| s.len() > 50)) {
                let spaces = "  ".repeat(indent + 1);
                let prefix = if has_prefix {
                    format!("\n{spaces}")
                } else {
                    " ".into()
                };
                (
                    format!("{}{} ", prefix, items.join(&format!(",\n{spaces}"))),
                    true,
                )
            } else {
                (items.join(", "), false)
            }
        }
    }
    Render {
        pretty,
        depth_limit,
        max_bytes,
    }
    .value(value, 0, 0, false)
    .map(|(s, _)| s)
}
