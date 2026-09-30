//! Typed values, bounded composite trees and scalar schema-value emission.
use crate::brand::Brand;
use crate::embed::Embedded;
use crate::resolve::Resolved;
use crate::syntax::{integer, Node, NodeKind, TokenKind};
use capnp::schema_capnp::value;
use std::sync::Arc;

type Result<T> = std::result::Result<T, &'static str>;
const MISMATCH: &str = "value does not match the field type or constant type, or is out of range";

#[derive(Clone, Debug)]
pub(crate) enum Value {
    Void,
    Bool(bool),
    Integer(i128),
    Float(f64),
    Text(Option<Arc<[u8]>>),
    Data(Option<Arc<[u8]>>),
    Enum(u16),
    Null,
    List(Arc<ListValue>),
    Struct(Arc<StructValue>),
}

#[derive(Debug)]
pub(crate) struct ListValue {
    pub element: Resolved,
    pub values: Vec<Value>,
    steps: usize,
    bytes: usize,
    height: usize,
}

#[derive(Debug)]
pub(crate) struct StructValue {
    pub node: usize,
    pub brand: Brand,
    // Keep source order: repeated assignments and inactive union storage have
    // observable wire contents in the reference compiler.
    pub fields: Vec<(usize, Value)>,
    pub embedded: Option<Arc<Embedded>>,
    steps: usize,
    bytes: usize,
    height: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct TypedValue {
    pub ty: Resolved,
    pub value: Value,
}

fn numeric(text: &str) -> Option<i128> {
    let (negative, magnitude) = text.strip_prefix('-').map_or((false, text), |s| (true, s));
    let magnitude = integer(magnitude)?;
    if negative {
        // C++ represents negative literals as signed 64-bit values, even when
        // the destination is floating point.
        (magnitude <= 1 << 63).then_some(-i128::from(magnitude))
    } else {
        Some(i128::from(magnitude))
    }
}

impl Value {
    pub fn implicit(ty: &Resolved) -> Self {
        match ty {
            Resolved::Void => Self::Void,
            Resolved::Bool => Self::Bool(false),
            Resolved::Float32 | Resolved::Float64 => Self::Float(0.0),
            Resolved::Text => Self::Text(None),
            Resolved::Data => Self::Data(None),
            Resolved::Enum(..) => Self::Enum(0),
            Resolved::List(_)
            | Resolved::Struct(..)
            | Resolved::Interface(..)
            | Resolved::AnyPointer
            | Resolved::Capability
            | Resolved::AnyStruct
            | Resolved::AnyList
            | Resolved::Parameter(..)
            | Resolved::ImplicitParameter(_) => Self::Null,
            Resolved::Namespace(_)
            | Resolved::ListConstructor
            | Resolved::Constant(..)
            | Resolved::Annotation(..) => {
                unreachable!("non-value type")
            }
            _ => Self::Integer(0),
        }
    }

    pub fn name(name: &str, ty: &Resolved, nodes: &[Node]) -> Option<Self> {
        if let Resolved::Enum(index, _) = ty {
            let NodeKind::Enum(enumerants) = &nodes[*index].kind else {
                unreachable!()
            };
            return enumerants
                .iter()
                .find(|e| e.name == name)
                .map(|e| Self::Enum(e.ordinal));
        }
        Some(match name {
            "void" => Self::Void,
            "true" => Self::Bool(true),
            "false" => Self::Bool(false),
            "inf" => Self::Float(f64::INFINITY),
            "nan" => Self::Float(f64::NAN),
            _ => return None,
        })
    }

    pub fn literal(token: &TokenKind, ty: &Resolved) -> Result<Self> {
        let value = match token {
            TokenKind::Number(text) => {
                if let Some(integer) = numeric(text) {
                    Self::Integer(integer)
                } else if text
                    .bytes()
                    .all(|b| b.is_ascii_digit() || b".-+eE".contains(&b))
                    && text.bytes().any(|b| b".eE".contains(&b))
                {
                    // The reference accepts overflow as +/- infinity.
                    Self::Float(text.parse::<f64>().map_err(|_| MISMATCH)?)
                } else {
                    return Err(MISMATCH);
                }
            }
            TokenKind::Name(name) if name == "-inf" => Self::Float(f64::NEG_INFINITY),
            TokenKind::Text(text) if matches!(ty, Resolved::Data) => {
                Self::Data(Some(Arc::from(text.as_slice())))
            }
            TokenKind::Text(text) => Self::Text(Some(Arc::from(text.as_slice()))),
            TokenKind::Data(data) if matches!(ty, Resolved::Data) => {
                Self::Data(Some(Arc::from(data.as_slice())))
            }
            _ => return Err(MISMATCH),
        };
        Ok(value)
    }

    pub fn coerce(self, ty: &Resolved) -> Result<Self> {
        match (&self, ty) {
            (Self::List(list), Resolved::List(element)) if list.element == **element => Ok(self),
            (Self::Struct(value), Resolved::Struct(node, brand))
                if value.node == *node && value.brand.equivalent(brand) =>
            {
                Ok(self)
            }
            (Self::List(_) | Self::Struct(_), Resolved::AnyPointer) => Ok(self),
            (Self::Struct(_), Resolved::AnyStruct) | (Self::List(_), Resolved::AnyList) => Ok(self),
            (Self::Void, Resolved::Void)
            | (Self::Bool(_), Resolved::Bool)
            | (Self::Text(_), Resolved::Text)
            | (Self::Data(_), Resolved::Data)
            | (Self::Enum(_), Resolved::Enum(..)) => Ok(self),
            (Self::Integer(n), Resolved::Float32) => Ok(Self::Float((*n as f32) as f64)),
            (Self::Integer(n), Resolved::Float64) => Ok(Self::Float(*n as f64)),
            (Self::Float(n), Resolved::Float32) => Ok(Self::Float((*n as f32) as f64)),
            (Self::Float(_), Resolved::Float64) => Ok(self),
            (Self::Integer(n), _) => {
                let (min, max) = match ty {
                    Resolved::Int8 => (i8::MIN as i128, i8::MAX as i128),
                    Resolved::Int16 => (i16::MIN as i128, i16::MAX as i128),
                    Resolved::Int32 => (i32::MIN as i128, i32::MAX as i128),
                    Resolved::Int64 => (i64::MIN as i128, i64::MAX as i128),
                    Resolved::UInt8 => (0, u8::MAX as i128),
                    Resolved::UInt16 => (0, u16::MAX as i128),
                    Resolved::UInt32 => (0, u32::MAX as i128),
                    Resolved::UInt64 => (0, u64::MAX as i128),
                    _ => return Err(MISMATCH),
                };
                if (min..=max).contains(n) {
                    Ok(self)
                } else {
                    Err(MISMATCH)
                }
            }
            _ => Err(MISMATCH),
        }
    }

    pub fn constant_reference(&self) -> Self {
        // The pinned C++ readConstant() exposes schema::Value.enum as its raw
        // UInt16 rather than a DynamicEnum. Preserve that observable behavior:
        // enum constants can feed integers, but not enum-typed defaults.
        match self {
            Self::Enum(ordinal) => Self::Integer(i128::from(*ordinal)),
            _ => self.clone(),
        }
    }

    fn height(&self) -> usize {
        match self {
            Self::List(v) => v.height,
            Self::Struct(v) => v.height,
            _ => 0,
        }
    }

    fn weight(&self) -> usize {
        match self {
            Self::List(list) => list.steps,
            Self::Struct(value) => value.steps,
            _ => 1,
        }
    }

    fn totals<'a>(values: impl Iterator<Item = &'a Value>) -> Result<(usize, usize, usize)> {
        let mut height = 1;
        let mut steps = 1usize;
        let mut bytes = 0usize;
        for value in values {
            height = height.max(value.height() + 1);
            if height >= 64 {
                return Err("expanded composite nesting limit exceeded (64)");
            }
            steps = steps.saturating_add(value.weight());
            bytes = bytes.saturating_add(value.pointer_bytes());
            if steps > 1_000_000 {
                return Err("expanded composite value work limit exceeded (1000000)");
            }
            if bytes > 16 * 1024 * 1024 {
                return Err("expanded value size limit exceeded (16 MiB)");
            }
        }
        Ok((steps, bytes, height))
    }

    pub fn list(element: Resolved, values: Vec<Value>) -> Result<Self> {
        let (steps, bytes, height) = Self::totals(values.iter())?;
        Ok(Self::List(Arc::new(ListValue {
            element,
            values,
            steps,
            bytes,
            height,
        })))
    }

    pub fn structure(node: usize, brand: Brand, fields: Vec<(usize, Value)>) -> Result<Self> {
        let (steps, bytes, height) = Self::totals(fields.iter().map(|(_, v)| v))?;
        Ok(Self::Struct(Arc::new(StructValue {
            node,
            brand,
            fields,
            embedded: None,
            steps,
            bytes,
            height,
        })))
    }

    pub fn embedded(node: usize, brand: Brand, file: &crate::embed::File) -> capnp::Result<Self> {
        let embedded = file.structure()?;
        Ok(Self::Struct(Arc::new(StructValue {
            node,
            brand,
            fields: Vec::new(),
            steps: embedded.size / 8 + 1,
            bytes: embedded.size,
            height: 1,
            embedded: Some(embedded),
        })))
    }

    pub fn bits(&self, ty: &Resolved) -> u64 {
        match self {
            Self::Void => 0,
            Self::Bool(v) => u64::from(*v),
            Self::Integer(v) => *v as u64,
            Self::Enum(v) => u64::from(*v),
            Self::Float(v) if matches!(ty, Resolved::Float32) => u64::from((*v as f32).to_bits()),
            Self::Float(v) => v.to_bits(),
            _ => unreachable!("pointer value has no scalar bits"),
        }
    }

    pub fn pointer_bytes(&self) -> usize {
        match self {
            Self::Text(Some(text)) => text.len() + 1,
            Self::Data(Some(data)) => data.len(),
            Self::List(list) => list.bytes,
            Self::Struct(value) => value.bytes,
            _ => 0,
        }
    }

    pub fn emit(&self, ty: &Resolved, mut builder: value::Builder<'_>) {
        match self {
            Self::List(_) | Self::Struct(_) => unreachable!("composites need a wire encoder"),
            Self::Void => builder.set_void(()),
            Self::Bool(value) => builder.set_bool(*value),
            Self::Enum(value) => builder.set_enum(*value),
            Self::Integer(value) => match ty {
                Resolved::Int8 => builder.set_int8(*value as i8),
                Resolved::Int16 => builder.set_int16(*value as i16),
                Resolved::Int32 => builder.set_int32(*value as i32),
                Resolved::Int64 => builder.set_int64(*value as i64),
                Resolved::UInt8 => builder.set_uint8(*value as u8),
                Resolved::UInt16 => builder.set_uint16(*value as u16),
                Resolved::UInt32 => builder.set_uint32(*value as u32),
                Resolved::UInt64 => builder.set_uint64(*value as u64),
                _ => unreachable!("integer value was type-checked"),
            },
            Self::Float(value) => match ty {
                Resolved::Float32 => builder.set_float32(*value as f32),
                Resolved::Float64 => builder.set_float64(*value),
                _ => unreachable!("float value was type-checked"),
            },
            Self::Text(Some(text)) => builder.set_text(capnp::text::Reader(text)),
            Self::Data(Some(data)) => builder.set_data(data.as_ref()),
            Self::Text(None) | Self::Data(None) => {
                let field = if matches!(self, Self::Text(_)) {
                    "text"
                } else {
                    "data"
                };
                let capnp::dynamic_value::Builder::Struct(mut value) = builder.into() else {
                    unreachable!()
                };
                value
                    .clear_named(field)
                    .expect("schema::Value pointer field");
            }
            Self::Null => match ty {
                Resolved::List(_) => {
                    builder.init_list();
                }
                Resolved::Struct(..) => {
                    builder.init_struct();
                }
                Resolved::Interface(..) => builder.set_interface(()),
                Resolved::AnyPointer
                | Resolved::Capability
                | Resolved::AnyStruct
                | Resolved::AnyList
                | Resolved::Parameter(..)
                | Resolved::ImplicitParameter(_) => {
                    builder.init_any_pointer();
                }
                _ => unreachable!("null pointer value was type-checked"),
            },
        }
    }
}

impl TypedValue {
    pub fn substitute(&self, brand: &Brand, depth: usize, budget: &mut usize) -> Result<Self> {
        let ty = self.ty.substitute(brand, depth, budget)?;
        let value = if ty == self.ty {
            self.value.clone()
        } else {
            self.value.substitute(brand, depth, budget)?
        };
        Ok(Self { ty, value })
    }
}

impl Value {
    fn substitute(&self, brand: &Brand, depth: usize, budget: &mut usize) -> Result<Self> {
        crate::brand::charge(depth, budget)?;
        match self {
            Self::Struct(value) if value.embedded.is_some() => {
                Ok(Self::Struct(Arc::new(StructValue {
                    node: value.node,
                    brand: value.brand.substitute(brand, depth + 1, budget)?,
                    fields: Vec::new(),
                    embedded: value.embedded.clone(),
                    steps: value.steps,
                    bytes: value.bytes,
                    height: value.height,
                })))
            }
            Self::Struct(value) => Self::structure(
                value.node,
                value.brand.substitute(brand, depth + 1, budget)?,
                value
                    .fields
                    .iter()
                    .map(|(i, v)| Ok((*i, v.substitute(brand, depth + 1, budget)?)))
                    .collect::<Result<_>>()?,
            ),
            Self::List(value) => Self::list(
                value.element.substitute(brand, depth + 1, budget)?,
                value
                    .values
                    .iter()
                    .map(|v| v.substitute(brand, depth + 1, budget))
                    .collect::<Result<_>>()?,
            ),
            _ => Ok(self.clone()),
        }
    }
}
