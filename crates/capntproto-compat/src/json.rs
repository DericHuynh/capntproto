//! Schema-driven JSON, with exact 64-bit integer strings and opt-in annotations.
mod value;
use crate::invalid;
use base64::Engine;
use capnp::{
    schema_capnp::{field, value as schema_value},
    schema_loader::{
        dynamic::{
            self,
            orphan::{Access, Editor, Orphan},
            HasMode,
        },
        AnnotationList, Field, Schema, Type,
    },
    Result,
};
use std::{
    cell::Cell,
    collections::{BTreeSet, HashMap, HashSet},
    rc::Rc,
};
pub use value::Value;

pub const NAME: u64 = 0xfa5b1fd61c2e7c3d;
pub const FLATTEN: u64 = 0x82d3e852af0336bf;
pub const DISCRIMINATOR: u64 = 0xcfa794e8d19a0162;
pub const BASE64: u64 = 0xd7d879450a253e4b;
pub const HEX: u64 = 0xf061e22f0ae5c7b5;
pub const NOTIFICATION: u64 = 0xa0a054dea32fd98c;

/// Application-defined type/field representations, including capabilities.
/// Decode allocates in the destination message's orphanage; ownership is checked on adoption.
pub trait Handler {
    fn encode(&self, codec: &JsonCodec, value: dynamic::Value<'_, '_>) -> Result<Value>;
    fn decode<'m, 's>(
        &self,
        codec: &JsonCodec,
        input: &Value,
        ty: Type<'s>,
        access: &mut Access<'_, 'm>,
    ) -> Result<Orphan<'m, 's>>;
}
type TypeHandler = (Box<dyn Fn(&Type<'_>) -> bool>, Rc<dyn Handler>);
type FieldHandler<'s> = (Schema<'s>, Rc<dyn Handler>);

pub struct JsonCodec<'s> {
    pub pretty_print: bool,
    pub has_mode: HasMode,
    pub reject_unknown_fields: bool,
    pub max_nesting_depth: usize,
    pub max_input_bytes: usize,
    pub max_output_bytes: usize,
    pub max_value_visits: usize,
    work: Cell<usize>,
    operations: Cell<usize>,
    annotated: HashMap<u64, Vec<Schema<'s>>>,
    fields: HashMap<(u64, u16), Vec<FieldHandler<'s>>>,
    types: Vec<TypeHandler>,
}
impl Default for JsonCodec<'_> {
    fn default() -> Self {
        Self {
            pretty_print: false,
            has_mode: HasMode::NonNull,
            reject_unknown_fields: false,
            max_nesting_depth: 64,
            max_input_bytes: 4 * 1024 * 1024,
            max_output_bytes: 16 * 1024 * 1024,
            max_value_visits: 100_000,
            work: Cell::new(100_000),
            operations: Cell::new(0),
            annotated: HashMap::new(),
            fields: HashMap::new(),
            types: vec![],
        }
    }
}
impl<'schema> JsonCodec<'schema> {
    pub fn new() -> Self {
        Self::default()
    }
    /// Match by full runtime type/brand, or use `Schema::extends` for interface subtypes.
    /// Later registrations take precedence. Matchers must not perform I/O.
    pub fn add_type_handler(
        &mut self,
        matches: impl Fn(&Type<'_>) -> bool + 'static,
        handler: Rc<dyn Handler>,
    ) {
        self.types.push((Box::new(matches), handler));
    }
    pub fn add_field_handler(&mut self, field: Field<'schema>, handler: Rc<dyn Handler>) {
        let entries = self
            .fields
            .entry((field.parent().id(), field.index()))
            .or_default();
        entries.retain(|(s, _)| !s.equals(&field.parent()));
        entries.push((field.parent(), handler));
    }
    fn field_handler(&self, field: &Field<'_>) -> Option<&Rc<dyn Handler>> {
        self.fields
            .get(&(field.parent().id(), field.index()))?
            .iter()
            .find(|(s, _)| s.equals(&field.parent()))
            .map(|(_, h)| h)
    }
    fn is_annotated(&self, schema: &Schema<'_>) -> bool {
        self.annotated
            .get(&schema.id())
            .is_some_and(|v| v.iter().any(|s| s.equals(schema)))
    }
    fn annotate(&mut self, schema: Schema<'schema>) -> bool {
        if self.is_annotated(&schema) {
            return false;
        }
        self.annotated.entry(schema.id()).or_default().push(schema);
        true
    }
    /// Scan reachable schemas now, including method parameters and results.
    pub fn handle_by_annotation(&mut self, schema: Schema<'schema>) -> Result<()> {
        self.work.set(self.max_value_visits);
        fn scan<'s>(codec: &mut JsonCodec<'s>, ty: Type<'s>, depth: usize) -> Result<()> {
            codec.depth(depth)?;
            match ty {
                Type::Struct(s) => {
                    if !codec.annotate(s.clone()) {
                        return Ok(());
                    }
                    let disc = discriminator(&s)?;
                    for f in s.fields()? {
                        let flattened = annotation(f.annotations()?, FLATTEN)?.is_some();
                        if flattened {
                            if !matches!(f.get_type()?, Type::Struct(_)) {
                                return Err(invalid("only struct fields can be flattened"));
                            }
                            if is_union(&f) && disc.is_none() {
                                return Err(invalid("flattened union needs a discriminator"));
                            }
                            if is_union(&f) && disc.as_ref().is_some_and(|(_, v)| v.is_some()) {
                                return Err(invalid("valueName and flatten cannot be combined"));
                            }
                        }
                        if (annotation(f.annotations()?, BASE64)?.is_some()
                            || annotation(f.annotations()?, HEX)?.is_some())
                            && f.get_type()? != Type::Data
                        {
                            return Err(invalid("base64/hex requires Data"));
                        }
                        scan(codec, f.get_type()?, depth + 1)?;
                    }
                    codec.validate_names(s, 0)?;
                }
                Type::Enum(s) => {
                    codec.annotate(s.clone());
                    let mut names = HashSet::new();
                    for e in s.enumerants()? {
                        if !names.insert(codec.name(
                            e.get_proto().get_name()?.to_str()?,
                            e.annotations()?,
                            s.clone(),
                        )?) {
                            return Err(invalid("duplicate JSON enumerant name"));
                        }
                    }
                }
                Type::Interface(s) => {
                    if !codec.annotate(s.clone()) {
                        return Ok(());
                    }
                    for m in s.methods()? {
                        scan(codec, Type::Struct(m.params()?), depth + 1)?;
                        scan(codec, Type::Struct(m.results()?), depth + 1)?;
                    }
                    for s in s.superclasses()? {
                        scan(codec, Type::Interface(s), depth + 1)?;
                    }
                }
                Type::List(t) => scan(codec, *t, depth + 1)?,
                _ => (),
            }
            Ok(())
        }
        let old = self.annotated.clone();
        let ty = match schema.kind() {
            capnp::schema_loader::Kind::Struct => Type::Struct(schema),
            capnp::schema_loader::Kind::Enum => Type::Enum(schema),
            capnp::schema_loader::Kind::Interface => Type::Interface(schema),
            _ => return Err(invalid("annotations need a struct, enum or interface")),
        };
        if let Err(e) = scan(self, ty, 0) {
            self.annotated = old;
            return Err(e);
        }
        Ok(())
    }
    fn depth(&self, depth: usize) -> Result<()> {
        self.work.set(
            self.work
                .get()
                .checked_sub(1)
                .ok_or_else(|| invalid("codec value work limit exceeded"))?,
        );
        if depth > self.max_nesting_depth.min(256) {
            Err(invalid("codec nesting limit exceeded"))
        } else {
            Ok(())
        }
    }
    pub fn parse(&self, input: &str) -> Result<Value> {
        value::parse(input, self.max_nesting_depth.min(256), self.max_input_bytes)
    }
    pub fn stringify(&self, value: &Value) -> Result<String> {
        value::render(
            value,
            self.pretty_print,
            self.max_nesting_depth.min(256),
            self.max_output_bytes,
        )
    }
    pub fn encode(&self, input: dynamic::Value<'_, '_>) -> Result<String> {
        self.stringify(&self.encode_value(input)?)
    }
    pub fn encode_value(&self, input: dynamic::Value<'_, '_>) -> Result<Value> {
        let _operation = self.operation();
        self.encode_at(input, None, 0)
    }
    pub fn decode(&self, input: &str, output: dynamic::Builder<'_, '_>) -> Result<()> {
        self.decode_value(&self.parse(input)?, output)
    }
    pub fn decode_value(&self, input: &Value, output: dynamic::Builder<'_, '_>) -> Result<()> {
        let _operation = self.operation();
        let ty = Type::Struct(output.schema());
        if let Some(handler) = self.handler(&ty, None) {
            let (mut output, orphanage) = output.with_orphanage();
            let owner = handler.decode(self, input, ty, &mut orphanage.in_struct(&mut output)?)?;
            return output.adopt_content(owner).map_err(|e| e.error);
        }
        self.fill_struct(input, output, 0).map(|_| ())
    }
    pub fn decode_orphan<'m, 's>(
        &self,
        input: &Value,
        ty: Type<'s>,
        access: &mut Access<'_, 'm>,
    ) -> Result<Orphan<'m, 's>> {
        let _operation = self.operation();
        self.decode_at(input, ty, None, access, 0)
    }
    fn operation(&self) -> Operation<'_, 'schema> {
        if self.operations.get() == 0 {
            self.work.set(self.max_value_visits);
        }
        self.operations.set(self.operations.get() + 1);
        Operation(self)
    }
    fn validate_names(&self, schema: Schema<'_>, depth: usize) -> Result<HashSet<String>> {
        self.depth(depth)?;
        let disc = self.disc(&schema)?;
        let mut names = HashMap::<String, Option<u16>>::new();
        if let Some((name, _)) = &disc {
            names.insert(name.clone(), None);
        }
        for f in schema.fields()? {
            self.depth(depth)?;
            let union = is_union(&f) && disc.is_some();
            let owner = union.then_some(f.index());
            let keys = if let Some(prefix) = self.field_prefix(&f)? {
                let Type::Struct(child) = f.get_type()? else {
                    return Err(invalid("flatten requires a struct"));
                };
                self.validate_names(child, depth + 1)?
                    .into_iter()
                    .map(|n| format!("{prefix}{n}"))
                    .collect::<Vec<_>>()
            } else if union && matches!(f.get_type()?, Type::Void) {
                vec![]
            } else {
                vec![if union {
                    disc.as_ref()
                        .and_then(|(_, v)| v.clone())
                        .unwrap_or(self.field_name(&f)?)
                } else {
                    self.field_name(&f)?
                }]
            };
            for key in keys {
                if let Some(old) = names.get(&key) {
                    if old.is_none() || owner.is_none() || *old == owner {
                        return Err(invalid(format!("conflicting JSON field name: {key}")));
                    }
                } else {
                    names.insert(key, owner);
                }
            }
        }
        Ok(names.into_keys().collect())
    }
    fn handler(&self, ty: &Type<'_>, field: Option<&Field<'_>>) -> Option<&dyn Handler> {
        field
            .and_then(|f| self.field_handler(f))
            .or_else(|| self.types.iter().rev().find(|(m, _)| m(ty)).map(|(_, h)| h))
            .map(|h| h.as_ref())
    }
    fn encode_at(
        &self,
        input: dynamic::Value<'_, '_>,
        field: Option<&Field<'_>>,
        depth: usize,
    ) -> Result<Value> {
        self.depth(depth)?;
        if let Some(handler) = self.handler(&type_of(&input), field) {
            return handler.encode(self, input);
        }
        if let Some(f) = field.filter(|f| self.is_annotated(&f.parent())) {
            if let dynamic::Value::Data(bytes) = input {
                if annotation(f.annotations()?, BASE64)?.is_some() {
                    return Ok(Value::String(
                        base64::engine::general_purpose::STANDARD.encode(bytes),
                    ));
                }
                if annotation(f.annotations()?, HEX)?.is_some() {
                    return Ok(Value::String(
                        bytes.iter().map(|b| format!("{b:02x}")).collect(),
                    ));
                }
            }
        }
        use dynamic::Value as D;
        Ok(match input {
            D::Void => Value::Null,
            D::Bool(v) => Value::Bool(v),
            D::Int8(v) => Value::Number(v.into()),
            D::Int16(v) => Value::Number(v.into()),
            D::Int32(v) => Value::Number(v.into()),
            D::UInt8(v) => Value::Number(v.into()),
            D::UInt16(v) => Value::Number(v.into()),
            D::UInt32(v) => Value::Number(v.into()),
            D::Int64(v) => Value::String(v.to_string()),
            D::UInt64(v) => Value::String(v.to_string()),
            D::Float32(v) => float(v.into()),
            D::Float64(v) => float(v),
            D::Text(v) => Value::String(v.to_str()?.into()),
            D::Data(v) => Value::Array(v.iter().map(|v| Value::Number((*v).into())).collect()),
            D::Enum(v, s) => {
                if let Some(e) = s.enumerants()?.into_iter().find(|e| e.index() == v) {
                    Value::String(self.name(
                        e.get_proto().get_name()?.to_str()?,
                        e.annotations()?,
                        s.clone(),
                    )?)
                } else {
                    Value::Number(v.into())
                }
            }
            D::List(v) => Value::Array(
                (0..v.len())
                    .map(|i| self.encode_at(v.get(i)?, None, depth + 1))
                    .collect::<Result<_>>()?,
            ),
            D::Struct(v) => self.encode_struct(v, depth + 1)?,
            D::AnyPointer(_) | D::Capability(_) => {
                return Err(invalid(
                    "AnyPointer/capability JSON requires a custom handler",
                ))
            }
            D::Unknown(_) => return Err(invalid("unknown dynamic JSON type")),
        })
    }
    fn name(
        &self,
        name: &str,
        annotations: AnnotationList<'_>,
        owner: Schema<'_>,
    ) -> Result<String> {
        if self.is_annotated(&owner) {
            if let Some(schema_value::Text(v)) = annotation(annotations, NAME)? {
                return Ok(v?.to_str()?.into());
            }
        }
        Ok(name.into())
    }
    fn field_name(&self, f: &Field<'_>) -> Result<String> {
        self.name(
            f.get_proto().get_name()?.to_str()?,
            f.annotations()?,
            f.parent(),
        )
    }
    fn field_prefix(&self, f: &Field<'_>) -> Result<Option<String>> {
        if !self.is_annotated(&f.parent()) {
            return Ok(None);
        }
        if let Some(schema_value::Struct(p)) = annotation(f.annotations()?, FLATTEN)? {
            Ok(Some(
                p.get_as::<crate::json_capnp::flatten_options::Reader>()?
                    .get_prefix()?
                    .to_str()?
                    .into(),
            ))
        } else {
            Ok(None)
        }
    }
    fn disc(&self, s: &Schema<'_>) -> Result<Option<(String, Option<String>)>> {
        if self.is_annotated(s) {
            discriminator(s)
        } else {
            Ok(None)
        }
    }
    fn encode_struct(&self, input: dynamic::Reader<'_, '_>, depth: usize) -> Result<Value> {
        self.depth(depth)?;
        let schema = input.schema();
        // A json.Value embedded in a message is itself a JSON value.
        if self.is_annotated(&schema)
            && schema.id()
                == <crate::json_capnp::value::Reader as capnp::traits::HasTypeId>::TYPE_ID
        {
            return self.encode_embedded(input, depth);
        }
        let disc = self.disc(&schema)?;
        let active = input.which()?;
        let mut fields = vec![];
        let mut ordered = schema.fields()?;
        if self.is_annotated(&schema) {
            ordered.sort_by_key(is_union);
        }
        for f in ordered {
            self.depth(depth)?;
            let union = is_union(&f);
            if union && active.as_ref().is_none_or(|a| a.index() != f.index()) {
                continue;
            }
            let present = input.has_with_mode(f.clone(), self.has_mode)?;
            if !present
                && (!union || (disc.is_none() && f.get_proto().get_discriminant_value() == 0))
            {
                continue;
            }
            if union {
                if let Some((name, _)) = &disc {
                    fields.push((name.clone(), Value::String(self.field_name(&f)?)));
                }
            }
            let value = if !present && disc.is_none() {
                Value::Null
            } else {
                self.encode_at(input.get(f.clone())?, Some(&f), depth + 1)?
            };
            if let Some(prefix) = self.field_prefix(&f)? {
                let Value::Object(children) = value else {
                    return Err(invalid("flatten handler must return an object"));
                };
                fields.extend(
                    children
                        .into_iter()
                        .map(|(k, v)| (format!("{prefix}{k}"), v)),
                );
            } else {
                let name = if union {
                    disc.as_ref()
                        .and_then(|(_, v)| v.clone())
                        .unwrap_or(self.field_name(&f)?)
                } else {
                    self.field_name(&f)?
                };
                if !(union && disc.is_some() && matches!(f.get_type()?, Type::Void)) {
                    fields.push((name, value));
                }
            }
        }
        Ok(Value::Object(fields))
    }
    fn decode_at<'m, 's>(
        &self,
        input: &Value,
        ty: Type<'s>,
        field: Option<&Field<'_>>,
        access: &mut Access<'_, 'm>,
        depth: usize,
    ) -> Result<Orphan<'m, 's>> {
        self.depth(depth)?;
        if let Some(handler) = self.handler(&ty, field) {
            return handler.decode(self, input, ty, access);
        }
        if matches!(input, Value::Null) && ty.is_pointer() {
            return access.null(ty);
        }
        if let Some(f) = field.filter(|f| self.is_annotated(&f.parent())) {
            if ty == Type::Data {
                if annotation(f.annotations()?, BASE64)?.is_some() {
                    let bytes = base64::engine::general_purpose::STANDARD
                        .decode(string(input)?)
                        .map_err(|e| invalid(e.to_string()))?;
                    return access.copy(dynamic::Value::Data(&bytes));
                }
                if annotation(f.annotations()?, HEX)?.is_some() {
                    let text = string(input)?;
                    if text.len() % 2 != 0 || !text.is_ascii() {
                        return Err(invalid("invalid hex data"));
                    }
                    let bytes = text
                        .as_bytes()
                        .chunks(2)
                        .map(|p| {
                            u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16)
                                .map_err(|_| invalid("invalid hex data"))
                        })
                        .collect::<Result<Vec<_>>>()?;
                    return access.copy(dynamic::Value::Data(&bytes));
                }
            }
        }
        match ty {
            Type::Struct(s) => {
                let mut orphan = access.new_struct(s)?;
                access.edit(&mut orphan, |e| match e {
                    Editor::Struct(b) => self.fill_struct(input, b, depth + 1).map(|_| ()),
                    _ => unreachable!(),
                })?;
                Ok(orphan)
            }
            Type::List(element) => {
                let Value::Array(values) = input else {
                    return Err(invalid("expected JSON array"));
                };
                let mut orphan = access.new_list(*element, count(values.len())?)?;
                access.edit(&mut orphan, |e| match e {
                    Editor::List(b) => self.fill_list(values, b, depth + 1),
                    _ => unreachable!(),
                })?;
                Ok(orphan)
            }
            Type::Text => access.copy(dynamic::Value::Text(string(input)?.into())),
            Type::Data => {
                let Value::Array(values) = input else {
                    return Err(invalid("expected byte array"));
                };
                let bytes = values
                    .iter()
                    .map(integer::<u8>)
                    .collect::<Result<Vec<_>>>()?;
                access.copy(dynamic::Value::Data(&bytes))
            }
            ty => access.copy(self.scalar(input, ty)?),
        }
    }
    fn scalar<'s>(&self, input: &Value, ty: Type<'s>) -> Result<dynamic::Value<'s, 's>> {
        use dynamic::Value as D;
        Ok(match ty {
            Type::Void if matches!(input, Value::Null) => D::Void,
            Type::Bool => {
                if let Value::Bool(v) = input {
                    D::Bool(*v)
                } else {
                    return Err(invalid("expected boolean"));
                }
            }
            Type::Int8 => D::Int8(integer(input)?),
            Type::Int16 => D::Int16(integer(input)?),
            Type::Int32 => D::Int32(integer(input)?),
            Type::Int64 => D::Int64(integer(input)?),
            Type::UInt8 => D::UInt8(integer(input)?),
            Type::UInt16 => D::UInt16(integer(input)?),
            Type::UInt32 => D::UInt32(integer(input)?),
            Type::UInt64 => D::UInt64(integer(input)?),
            Type::Float32 => D::Float32(number(input)? as f32),
            Type::Float64 => D::Float64(number(input)?),
            Type::Enum(s) => {
                let n = if let Value::String(name) = input {
                    let mut found = None;
                    for e in s.enumerants()? {
                        if self.name(
                            e.get_proto().get_name()?.to_str()?,
                            e.annotations()?,
                            s.clone(),
                        )? == *name
                        {
                            found = Some(e.index());
                            break;
                        }
                    }
                    found.ok_or_else(|| invalid("unknown enumerant"))?
                } else {
                    return Err(invalid("expected enum name"));
                };
                D::Enum(n, s)
            }
            _ => {
                return Err(invalid(
                    "JSON value does not match schema type (capabilities/AnyPointer need handlers)",
                ))
            }
        })
    }
    fn fill_list(
        &self,
        values: &[Value],
        output: dynamic::ListBuilder<'_, '_>,
        depth: usize,
    ) -> Result<()> {
        self.depth(depth)?;
        let ty = output.as_reader().element_type();
        let (mut output, orphanage) = output.with_orphanage();
        for (i, v) in values.iter().enumerate() {
            let owner = self.decode_at(
                v,
                ty.clone(),
                None,
                &mut orphanage.in_list(&mut output)?,
                depth + 1,
            )?;
            output.adopt(i as u32, owner).map_err(|e| e.error)?;
        }
        Ok(())
    }
    fn fill_struct(
        &self,
        input: &Value,
        output: dynamic::Builder<'_, '_>,
        depth: usize,
    ) -> Result<BTreeSet<String>> {
        self.fill_struct_inner(input, output, depth, self.reject_unknown_fields)
    }
    fn fill_struct_inner(
        &self,
        input: &Value,
        output: dynamic::Builder<'_, '_>,
        depth: usize,
        reject: bool,
    ) -> Result<BTreeSet<String>> {
        self.depth(depth)?;
        let schema = output.schema();
        if self.is_annotated(&schema)
            && schema.id()
                == <crate::json_capnp::value::Reader as capnp::traits::HasTypeId>::TYPE_ID
        {
            self.decode_embedded(input, output, depth)?;
            return Ok(BTreeSet::new());
        }
        let Value::Object(entries) = input else {
            return Err(invalid("expected JSON object"));
        };
        let disc = self.disc(&schema)?;
        let fields = schema.fields()?;
        let mut by_name = HashMap::<&str, Vec<&Value>>::new();
        for (name, value) in entries {
            self.depth(depth)?;
            by_name.entry(name).or_default().push(value);
        }
        let mut consumed = BTreeSet::new();
        let selected = if let Some((name, _)) = &disc {
            if let Some(value) = input.get(name) {
                consumed.insert(name.clone());
                let name = string(value)?;
                let mut selected = None;
                for f in &fields {
                    if is_union(f) && self.field_name(f)? == name {
                        selected = Some(f.index());
                    }
                }
                Some(selected.ok_or_else(|| invalid("unknown union discriminator"))?)
            } else {
                None
            }
        } else {
            None
        };
        let (mut output, orphanage) = output.with_orphanage();
        if let Some(index) = selected {
            output.clear(fields.iter().find(|f| f.index() == index).unwrap().clone())?;
        }
        for f in fields {
            self.depth(depth)?;
            let union = is_union(&f);
            if union && disc.is_some() && selected != Some(f.index()) {
                continue;
            }
            if let Some(prefix) = self.field_prefix(&f)? {
                let subset = Value::Object(
                    entries
                        .iter()
                        .filter_map(|(k, v)| k.strip_prefix(&prefix).map(|s| (s.into(), v.clone())))
                        .collect(),
                );
                let used = self.fill_struct_inner(
                    &subset,
                    output
                        .reborrow()
                        .get_struct(f.get_proto().get_name()?.to_str()?)?,
                    depth + 1,
                    false,
                )?;
                consumed.extend(used.into_iter().map(|k| format!("{prefix}{k}")));
            } else {
                let name = if union {
                    disc.as_ref()
                        .and_then(|(_, v)| v.clone())
                        .unwrap_or(self.field_name(&f)?)
                } else {
                    self.field_name(&f)?
                };
                for v in by_name.get(name.as_str()).into_iter().flatten().copied() {
                    consumed.insert(name.clone());
                    // C++ treats JSON null pointer fields as absent, retaining any
                    // existing value and union selection. Field handlers override this.
                    if matches!(v, Value::Null)
                        && matches!(
                            f.get_type()?,
                            Type::Text | Type::Data | Type::List(_) | Type::Struct(_)
                        )
                        && self.field_handler(&f).is_none()
                    {
                        continue;
                    }
                    let owner = self.decode_at(
                        v,
                        f.get_type()?,
                        Some(&f),
                        &mut orphanage.in_struct(&mut output)?,
                        depth + 1,
                    )?;
                    output.adopt(f.clone(), owner).map_err(|e| e.error)?;
                }
            }
        }
        if reject {
            for (name, _) in entries {
                if !consumed.contains(name) {
                    return Err(invalid(format!("unknown JSON field: {name}")));
                }
            }
        }
        Ok(consumed)
    }
    fn encode_embedded(&self, input: dynamic::Reader<'_, '_>, depth: usize) -> Result<Value> {
        let f = input
            .which()?
            .ok_or_else(|| invalid("unknown json.Value arm"))?;
        let name = f.get_proto().get_name()?.to_str()?;
        let v = input.get(f)?;
        Ok(match (name, v) {
            ("null", _) => Value::Null,
            ("boolean", dynamic::Value::Bool(b)) => Value::Bool(b),
            ("number", dynamic::Value::Float64(v)) => Value::Number(v),
            ("string", dynamic::Value::Text(v)) => Value::String(v.to_str()?.into()),
            ("raw", dynamic::Value::Text(v)) => Value::Raw(v.to_str()?.into()),
            ("array", dynamic::Value::List(v)) => Value::Array(
                (0..v.len())
                    .map(|i| self.encode_at(v.get(i)?, None, depth + 1))
                    .collect::<Result<_>>()?,
            ),
            ("object", dynamic::Value::List(v)) => {
                let mut fields = vec![];
                for i in 0..v.len() {
                    let dynamic::Value::Struct(f) = v.get(i)? else {
                        unreachable!()
                    };
                    let dynamic::Value::Text(name) = f.get_named("name")? else {
                        unreachable!()
                    };
                    fields.push((
                        name.to_str()?.into(),
                        self.encode_at(f.get_named("value")?, None, depth + 1)?,
                    ));
                }
                Value::Object(fields)
            }
            ("call", dynamic::Value::Struct(v)) => {
                let dynamic::Value::Text(name) = v.get_named("function")? else {
                    unreachable!()
                };
                let Value::Array(params) =
                    self.encode_at(v.get_named("params")?, None, depth + 1)?
                else {
                    unreachable!()
                };
                Value::Call {
                    function: name.to_str()?.into(),
                    params,
                }
            }
            _ => return Err(invalid("invalid json.Value")),
        })
    }
    fn decode_embedded(
        &self,
        input: &Value,
        mut output: dynamic::Builder<'_, '_>,
        depth: usize,
    ) -> Result<()> {
        self.depth(depth)?;
        use dynamic::Value as D;
        match input {
            Value::Null => output.set_named("null", D::Void),
            Value::Bool(b) => output.set_named("boolean", D::Bool(*b)),
            Value::Number(n) => output.set_named("number", D::Float64(*n)),
            Value::String(s) => output.set_named("string", D::Text(s.as_str().into())),
            Value::Raw(s) => output.set_named("raw", D::Text(s.as_str().into())),
            Value::Array(v) => {
                let b = output.init_list("array", count(v.len())?)?;
                self.fill_list(v, b, depth + 1)
            }
            Value::Object(fields) => {
                let mut list = output.init_list("object", count(fields.len())?)?;
                for (i, (name, v)) in fields.iter().enumerate() {
                    let mut f = list.reborrow().get_struct(i as u32)?;
                    f.set_named("name", D::Text(name.as_str().into()))?;
                    self.decode_embedded(v, f.init_struct("value")?, depth + 1)?;
                }
                Ok(())
            }
            Value::Call { function, params } => {
                let mut b = output.init_struct("call")?;
                b.set_named("function", D::Text(function.as_str().into()))?;
                self.fill_list(
                    params,
                    b.init_list("params", count(params.len())?)?,
                    depth + 1,
                )
            }
        }
    }
}

pub(crate) fn annotation(
    list: AnnotationList<'_>,
    id: u64,
) -> Result<Option<schema_value::WhichReader<'_>>> {
    list.find(id)?
        .map(|a| a.get_proto().get_value()?.which().map_err(Into::into))
        .transpose()
}
fn discriminator(s: &Schema<'_>) -> Result<Option<(String, Option<String>)>> {
    if let Some(schema_value::Struct(p)) = annotation(s.annotations()?, DISCRIMINATOR)? {
        let v = p.get_as::<crate::json_capnp::discriminator_options::Reader>()?;
        let name = if v.has_name() {
            v.get_name()?.to_str()?.into()
        } else {
            s.unqualified_name()?.to_str()?.into()
        };
        Ok(Some((
            name,
            if v.has_value_name() {
                Some(v.get_value_name()?.to_str()?.into())
            } else {
                None
            },
        )))
    } else {
        Ok(None)
    }
}
fn is_union(f: &Field<'_>) -> bool {
    f.get_proto().get_discriminant_value() != field::NO_DISCRIMINANT
}
pub(crate) fn string(v: &Value) -> Result<&str> {
    v.as_str().ok_or_else(|| invalid("expected string"))
}
fn count(n: usize) -> Result<u32> {
    u32::try_from(n).map_err(|_| invalid("list length overflow"))
}
fn float(n: f64) -> Value {
    if n.is_nan() {
        Value::String("NaN".into())
    } else if n == f64::INFINITY {
        Value::String("Infinity".into())
    } else if n == f64::NEG_INFINITY {
        Value::String("-Infinity".into())
    } else {
        Value::Number(n)
    }
}
pub(crate) fn number(v: &Value) -> Result<f64> {
    match v {
        Value::Number(n) => Ok(*n),
        Value::Null => Ok(f64::NAN),
        Value::String(s) => s
            .parse()
            .map_err(|_| invalid("invalid floating-point string")),
        _ => Err(invalid("expected number")),
    }
}
fn integer<T: std::str::FromStr>(v: &Value) -> Result<T> {
    match v {
        Value::String(s) => s
            .parse()
            .map_err(|_| invalid("integer string invalid or out of range")),
        Value::Number(n) if n.is_finite() && n.fract() == 0.0 => format!("{n:.0}")
            .parse()
            .map_err(|_| invalid("integer out of range")),
        _ => Err(invalid("expected integral number")),
    }
}
fn type_of<'s>(v: &dynamic::Value<'_, 's>) -> Type<'s> {
    use dynamic::Value as D;
    match v {
        D::Void => Type::Void,
        D::Bool(_) => Type::Bool,
        D::Int8(_) => Type::Int8,
        D::Int16(_) => Type::Int16,
        D::Int32(_) => Type::Int32,
        D::Int64(_) => Type::Int64,
        D::UInt8(_) => Type::UInt8,
        D::UInt16(_) => Type::UInt16,
        D::UInt32(_) => Type::UInt32,
        D::UInt64(_) => Type::UInt64,
        D::Float32(_) => Type::Float32,
        D::Float64(_) => Type::Float64,
        D::Text(_) => Type::Text,
        D::Data(_) => Type::Data,
        D::Struct(s) => Type::Struct(s.schema()),
        D::List(l) => Type::List(Box::new(l.element_type())),
        D::Enum(_, s) => Type::Enum(s.clone()),
        D::Capability(c) => c.schema().map(Type::Interface).unwrap_or(Type::AnyPointer(
            capnp::schema_loader::PointerKind::Capability,
        )),
        D::AnyPointer(_) => Type::AnyPointer(capnp::schema_loader::PointerKind::Any),
        D::Unknown(n) => Type::Unknown(*n),
    }
}

struct Operation<'a, 's>(&'a JsonCodec<'s>);
impl Drop for Operation<'_, '_> {
    fn drop(&mut self) {
        self.0.operations.set(self.0.operations.get() - 1);
    }
}
