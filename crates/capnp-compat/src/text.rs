//! Cap'n Proto text values, using the same bounded lexer as the Rust schema compiler.
use crate::{
    invalid,
    json::{JsonCodec, Value as Json},
};
use capnp::{
    schema_loader::{
        dynamic::{
            self,
            orphan::{Access, Editor, Orphan},
            HasMode,
        },
        Type,
    },
    Result,
};
use capnp_compiler::literal::{parse_literal, Literal};

#[derive(Default)]
pub struct TextCodec {
    pub pretty_print: bool,
}
impl TextCodec {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn encode(&self, value: dynamic::Value<'_, '_>) -> Result<String> {
        self.render(value, 0, false)
    }
    pub fn decode(&self, input: &str, output: dynamic::Builder<'_, '_>) -> Result<()> {
        self.fill_struct(
            &parse_literal(input).map_err(|e| invalid(e.to_string()))?,
            output,
            0,
        )
    }
    pub fn decode_orphan<'m, 's>(
        &self,
        input: &str,
        ty: Type<'s>,
        access: &mut Access<'_, 'm>,
    ) -> Result<Orphan<'m, 's>> {
        self.read(
            &parse_literal(input).map_err(|e| invalid(e.to_string()))?,
            ty,
            access,
            0,
        )
    }
    fn read<'m, 's>(
        &self,
        input: &Literal,
        ty: Type<'s>,
        access: &mut Access<'_, 'm>,
        depth: usize,
    ) -> Result<Orphan<'m, 's>> {
        if depth > 64 {
            return Err(invalid("text nesting limit exceeded"));
        }
        match (input, ty) {
            (Literal::Text(bytes), Type::Text) => {
                access.copy(dynamic::Value::Text(bytes.as_slice().into()))
            }
            (Literal::Text(bytes) | Literal::Data(bytes), Type::Data) => {
                access.copy(dynamic::Value::Data(bytes))
            }
            (Literal::Struct(_), Type::Struct(s)) => {
                let mut owner = access.new_struct(s)?;
                access.edit(&mut owner, |e| match e {
                    Editor::Struct(b) => self.fill_struct(input, b, depth + 1),
                    _ => unreachable!(),
                })?;
                Ok(owner)
            }
            (Literal::List(values), Type::List(element)) => {
                let mut owner = access.new_list(
                    *element,
                    u32::try_from(values.len()).map_err(|_| invalid("list too long"))?,
                )?;
                access.edit(&mut owner, |e| match e {
                    Editor::List(b) => self.fill_list(values, b, depth + 1),
                    _ => unreachable!(),
                })?;
                Ok(owner)
            }
            (Literal::Name(n), ty) if n == "null" && ty.is_pointer() => access.null(ty),
            (value, Type::Struct(schema)) => {
                let mut owner = access.new_struct(schema)?;
                access.edit(&mut owner, |e| match e {
                    Editor::Struct(b) => self.fill_implicit_struct(value, b, depth + 1),
                    _ => unreachable!(),
                })?;
                Ok(owner)
            }
            (Literal::Number(value), ty) => access.copy(number(value, ty)?),
            (value, ty) => {
                let json =
                    match value {
                        Literal::Name(v) if matches!(ty, Type::Void) && v == "void" => Json::Null,
                        Literal::Name(v)
                            if matches!(ty, Type::Bool) && (v == "true" || v == "false") =>
                        {
                            Json::Bool(v == "true")
                        }
                        Literal::Name(v) if matches!(ty, Type::Enum(_)) => Json::String(v.clone()),
                        Literal::Name(v)
                            if matches!(ty, Type::Float32 | Type::Float64)
                                && ["nan", "inf", "-inf"].contains(&v.as_str()) =>
                        {
                            Json::String(v.clone())
                        }
                        _ => return Err(invalid(
                            "text value does not match schema; external constants are not allowed",
                        )),
                    };
                JsonCodec::new().decode_orphan(&json, ty, access)
            }
        }
    }
    fn fill_implicit_struct(
        &self,
        value: &Literal,
        output: dynamic::Builder<'_, '_>,
        depth: usize,
    ) -> Result<()> {
        if depth > 64 {
            return Err(invalid("text nesting limit exceeded"));
        }
        let field = output
            .schema()
            .fields()?
            .into_iter()
            .next()
            .ok_or_else(|| invalid("cannot implicitly wrap a value in an empty struct"))?;
        let ty = field.get_type()?;
        // C++ evaluates the literal with the outer struct as its type hint,
        // then matches the resulting scalar to the first field. Do not re-infer
        // enum/list/Data values using that field's type, or unwrap a second
        // struct/group. The schema's field order is independent of source order.
        let unambiguous = match value {
            Literal::Number(_) => true,
            Literal::Text(_) => matches!(ty, Type::Text),
            Literal::Name(n) => {
                ["void", "true", "false", "inf", "-inf", "nan"].contains(&n.as_str())
                    && !matches!(ty, Type::Enum(_))
            }
            _ => false,
        };
        if !unambiguous || matches!(ty, Type::Struct(_)) {
            return Err(invalid(
                "implicit struct conversion requires a matching scalar first field",
            ));
        }
        let (mut output, orphanage) = output.with_orphanage();
        let scalar = self.read(value, ty, &mut orphanage.in_struct(&mut output)?, depth + 1)?;
        output.adopt(field, scalar).map_err(|e| e.error)
    }

    fn fill_struct(
        &self,
        input: &Literal,
        output: dynamic::Builder<'_, '_>,
        depth: usize,
    ) -> Result<()> {
        if depth > 64 {
            return Err(invalid("text nesting limit exceeded"));
        }
        let Literal::Struct(fields) = input else {
            return Err(invalid("expected text struct"));
        };
        let schema = output.schema();
        let (mut output, orphanage) = output.with_orphanage();
        for (name, value) in fields {
            let f = schema.field(name)?;
            match f.get_proto().which()? {
                capnp::schema_capnp::field::Slot(_) => {
                    // Evaluate before adoption: a failed replacement retains the
                    // previous slot and union selection. Assign in source order.
                    let owner = self.read(
                        value,
                        f.get_type()?,
                        &mut orphanage.in_struct(&mut output)?,
                        depth + 1,
                    )?;
                    output.adopt(f, owner).map_err(|e| e.error)?;
                }
                capnp::schema_capnp::field::Group(_) => {
                    // C++ initializes groups in place, including union activation,
                    // before evaluating the value. Errors expose the reset group
                    // and any successfully assigned prefix of its fields.
                    let group = output.reborrow().init_struct(name)?;
                    if matches!(value, Literal::Struct(_)) {
                        self.fill_struct(value, group, depth + 2)?;
                    } else {
                        self.fill_implicit_struct(value, group, depth + 2)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn fill_list(
        &self,
        values: &[Literal],
        output: dynamic::ListBuilder<'_, '_>,
        depth: usize,
    ) -> Result<()> {
        let ty = output.as_reader().element_type();
        let (mut output, orphanage) = output.with_orphanage();
        for (i, value) in values.iter().enumerate() {
            let owner = self.read(
                value,
                ty.clone(),
                &mut orphanage.in_list(&mut output)?,
                depth + 1,
            )?;
            output.adopt(i as u32, owner).map_err(|e| e.error)?;
        }
        Ok(())
    }
    fn render(
        &self,
        value: dynamic::Value<'_, '_>,
        depth: usize,
        prefixed: bool,
    ) -> Result<String> {
        if depth > 64 {
            return Err(invalid("text nesting limit exceeded"));
        }
        use dynamic::Value as V;
        let output = match value {
            V::Void => "void".into(),
            V::Bool(v) => v.to_string(),
            V::Int8(v) => v.to_string(),
            V::Int16(v) => v.to_string(),
            V::Int32(v) => v.to_string(),
            V::Int64(v) => v.to_string(),
            V::UInt8(v) => v.to_string(),
            V::UInt16(v) => v.to_string(),
            V::UInt32(v) => v.to_string(),
            V::UInt64(v) => v.to_string(),
            V::Float32(v) => crate::format::float32(v),
            V::Float64(v) => crate::format::float64(v),
            V::Text(v) => {
                let mut s = String::new();
                quoted(v.as_bytes(), &mut s, false);
                s
            }
            V::Data(v) => {
                let mut s = String::new();
                quoted(v, &mut s, true);
                s
            }
            V::Enum(n, s) => {
                if let Some(e) = s.enumerants()?.into_iter().find(|e| e.index() == n) {
                    e.get_proto().get_name()?.to_str()?.into()
                } else {
                    format!("({n})")
                }
            }
            V::List(v) => {
                let mut items = vec![];
                for i in 0..v.len() {
                    items.push(self.render(v.get(i)?, depth + 1, false)?);
                }
                format!("[{}]", self.delimit(items, depth, prefixed, false))
            }
            V::Struct(v) => {
                let mut items = vec![];
                let active = v.which()?;
                for f in v.schema().fields()? {
                    let tag = f.get_proto().get_discriminant_value();
                    let union = tag != capnp::schema_capnp::field::NO_DISCRIMINANT;
                    if union && active.as_ref().is_none_or(|a| a.index() != f.index()) {
                        continue;
                    }
                    if (!union || tag == 0) && !v.has_with_mode(f.clone(), HasMode::NonNull)? {
                        continue;
                    }
                    items.push(format!(
                        "{} = {}",
                        f.get_proto().get_name()?.to_str()?,
                        self.render(v.get(f)?, depth + 1, true)?
                    ));
                }
                format!("({})", self.delimit(items, depth, prefixed, true))
            }
            V::AnyPointer(_) => "<opaque pointer>".into(),
            V::Capability(_) => "<external capability>".into(),
            V::Unknown(_) => "?".into(),
        };
        if output.len() > 16 * 1024 * 1024 {
            return Err(invalid("text output size limit exceeded"));
        }
        Ok(output)
    }
    fn delimit(&self, items: Vec<String>, depth: usize, prefixed: bool, record: bool) -> String {
        if !self.pretty_print
            || (items.iter().all(|s| s.len() <= 24 && !s.contains('\n'))
                && (!record || items.iter().map(String::len).sum::<usize>() <= 64))
        {
            return items.join(", ");
        }
        let indent = "  ".repeat(depth + 1);
        format!(
            "{}{} ",
            if prefixed {
                format!("\n{indent}")
            } else {
                " ".into()
            },
            items.join(&format!(",\n{indent}"))
        )
    }
}

// Preserve integer-vs-float syntax until after type checking. JSON's string
// coercions can turn numbers into Text and round integers via f64; neither
// behavior matches Cap'n Proto text values. Enum ordinals are not numeric inputs.
fn number<'s>(v: &str, ty: Type<'s>) -> Result<dynamic::Value<'s, 's>> {
    use dynamic::Value as V;
    let (negative, s) = v.strip_prefix('-').map_or((false, v), |s| (true, s));
    if !s.starts_with("0x") && s.bytes().any(|b| b".eE".contains(&b)) {
        let n = v
            .parse::<f64>()
            .map_err(|_| invalid("invalid floating-point literal"))?;
        return match ty {
            Type::Float32 => Ok(V::Float32(n as f32)),
            Type::Float64 => Ok(V::Float64(n)),
            _ => Err(invalid(
                "floating-point literal requires a floating-point type",
            )),
        };
    }
    let (radix, digits) = if let Some(hex) = s.strip_prefix("0x") {
        (16, hex)
    } else if s.len() > 1 && s.starts_with('0') {
        (8, &s[1..])
    } else {
        (10, s)
    };
    let n = u64::from_str_radix(digits, radix)
        .map_err(|_| invalid("invalid integer or out of range"))?;
    let n = if negative {
        if n > 1_u64 << 63 {
            return Err(invalid("negative integer out of range"));
        }
        -i128::from(n)
    } else {
        i128::from(n)
    };
    let range = |_| invalid("integer out of range for schema type");
    Ok(match ty {
        Type::Int8 => V::Int8(n.try_into().map_err(range)?),
        Type::Int16 => V::Int16(n.try_into().map_err(range)?),
        Type::Int32 => V::Int32(n.try_into().map_err(range)?),
        Type::Int64 => V::Int64(n.try_into().map_err(range)?),
        Type::UInt8 => V::UInt8(n.try_into().map_err(range)?),
        Type::UInt16 => V::UInt16(n.try_into().map_err(range)?),
        Type::UInt32 => V::UInt32(n.try_into().map_err(range)?),
        Type::UInt64 => V::UInt64(n.try_into().map_err(range)?),
        Type::Float32 => V::Float32(n as f32),
        Type::Float64 => V::Float64(n as f64),
        _ => return Err(invalid("integer literal requires a numeric type")),
    })
}
fn quoted(bytes: &[u8], out: &mut String, binary: bool) {
    out.push('"');
    let text = if binary {
        None
    } else {
        std::str::from_utf8(bytes).ok()
    };
    if let Some(text) = text {
        for ch in text.chars() {
            if ch.is_ascii() {
                quoted_byte(ch as u8, out);
            } else {
                out.push(ch);
            }
        }
    } else {
        // Rust's output is a UTF-8 String. Preserve malformed Text bytes with
        // escapes; C++'s byte-string API can emit them literally. Data always
        // uses octal escapes for high bytes, even if they form valid UTF-8.
        for &byte in bytes {
            quoted_byte(byte, out);
        }
    }
    out.push('"');
}

fn quoted_byte(byte: u8, out: &mut String) {
    match byte {
        7 => out.push_str("\\a"),
        8 => out.push_str("\\b"),
        12 => out.push_str("\\f"),
        b'\n' => out.push_str("\\n"),
        b'\r' => out.push_str("\\r"),
        b'\t' => out.push_str("\\t"),
        11 => out.push_str("\\v"),
        b'\'' => out.push_str("\\'"),
        b'"' => out.push_str("\\\""),
        b'\\' => out.push_str("\\\\"),
        0x20..=0x7e => out.push(byte as char),
        _ => out.push_str(&format!("\\{byte:03o}")),
    }
}
