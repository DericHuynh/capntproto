use super::*;
use crate::dynamic_value::{
    conversion::{mismatch, Number},
    Reader as Compiled,
};
use crate::introspect::TypeVariant as T;

impl<'a, 's: 'a> Value<'a, 's> {
    /// Explicit checked conversion with the same numeric, enum-name/ordinal and
    /// Text-to-Data rules as `dynamic_value::Reader::try_convert()`.
    /// Aggregates preserve exact schema/brand identity; capability interfaces
    /// may be upcast. Unknown types and symbolic parameters are not converted.
    /// Convert before invoking a setter to preserve the destination on error.
    pub fn try_convert(self, target: &Type<'s>) -> Result<Self> {
        let number = match &self {
            Self::Int8(v) => Some(Number::Signed(i64::from(*v))),
            Self::Int16(v) => Some(Number::Signed(i64::from(*v))),
            Self::Int32(v) => Some(Number::Signed(i64::from(*v))),
            Self::Int64(v) => Some(Number::Signed(*v)),
            Self::UInt8(v) => Some(Number::Unsigned(u64::from(*v))),
            Self::UInt16(v) => Some(Number::Unsigned(u64::from(*v))),
            Self::UInt32(v) => Some(Number::Unsigned(u64::from(*v))),
            Self::UInt64(v) => Some(Number::Unsigned(*v)),
            Self::Float32(v) => Some(Number::Float(f64::from(*v))),
            Self::Float64(v) => Some(Number::Float(*v)),
            _ => None,
        };
        if let Some(number) = number {
            if let Type::Enum(schema) = target {
                require(schema.kind() == Kind::Enum, "not an enum schema")?;
                return Ok(Self::Enum(number.ordinal()?, schema.clone()));
            }
            let primitive = match target {
                Type::Int8 => T::Int8,
                Type::Int16 => T::Int16,
                Type::Int32 => T::Int32,
                Type::Int64 => T::Int64,
                Type::UInt8 => T::UInt8,
                Type::UInt16 => T::UInt16,
                Type::UInt32 => T::UInt32,
                Type::UInt64 => T::UInt64,
                Type::Float32 => T::Float32,
                Type::Float64 => T::Float64,
                _ => return Err(mismatch()),
            };
            return Ok(match number.convert(primitive)? {
                Compiled::Int8(v) => Self::Int8(v),
                Compiled::Int16(v) => Self::Int16(v),
                Compiled::Int32(v) => Self::Int32(v),
                Compiled::Int64(v) => Self::Int64(v),
                Compiled::UInt8(v) => Self::UInt8(v),
                Compiled::UInt16(v) => Self::UInt16(v),
                Compiled::UInt32(v) => Self::UInt32(v),
                Compiled::UInt64(v) => Self::UInt64(v),
                Compiled::Float32(v) => Self::Float32(v),
                Compiled::Float64(v) => Self::Float64(v),
                _ => unreachable!(),
            });
        }
        match (self, target) {
            (Self::Text(v), Type::Enum(schema)) => {
                for member in schema.enumerants()? {
                    if member.get_proto().get_name()?.as_bytes() == v.as_bytes() {
                        return Ok(Self::Enum(member.index(), schema.clone()));
                    }
                }
                Err(mismatch())
            }
            (Self::Text(v), Type::Data) => Ok(Self::Data(v.as_bytes())),
            (Self::Capability(mut v), Type::Interface(schema)) => {
                require(schema.kind() == Kind::Interface, "not an interface schema")?;
                if let Some(source) = &v.schema {
                    require(source.extends(schema)?, "capability interface mismatch")?;
                }
                v.schema = Some(schema.clone());
                Ok(Self::Capability(v))
            }
            (_, Type::Unknown(_) | Type::Parameter(..)) => Err(mismatch()),
            (value, _) => {
                value.check(target)?;
                Ok(value)
            }
        }
    }
}
