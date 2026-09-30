//! Shared numeric rules for compiled and loaded reflection.
use super::{Enum, Reader};
use crate::{
    introspect::{Type, TypeVariant as T},
    Error, ErrorKind, Result,
};

pub(crate) fn mismatch() -> Error {
    Error::from_kind(ErrorKind::TypeMismatch)
}
fn range_error() -> Error {
    Error::from_kind(ErrorKind::NumericConversionOutOfRange)
}

#[derive(Clone, Copy)]
pub(crate) enum Number {
    Signed(i64),
    Unsigned(u64),
    Float(f64),
}
impl Number {
    pub(crate) fn from_reader(value: &Reader<'_>) -> Option<Self> {
        Some(match value {
            Reader::Int8(v) => Self::Signed(i64::from(*v)),
            Reader::Int16(v) => Self::Signed(i64::from(*v)),
            Reader::Int32(v) => Self::Signed(i64::from(*v)),
            Reader::Int64(v) => Self::Signed(*v),
            Reader::UInt8(v) => Self::Unsigned(u64::from(*v)),
            Reader::UInt16(v) => Self::Unsigned(u64::from(*v)),
            Reader::UInt32(v) => Self::Unsigned(u64::from(*v)),
            Reader::UInt64(v) => Self::Unsigned(*v),
            Reader::Float32(v) => Self::Float(f64::from(*v)),
            Reader::Float64(v) => Self::Float(*v),
            _ => return None,
        })
    }
    fn integer(self) -> Result<i128> {
        match self {
            Self::Signed(v) => Ok(i128::from(v)),
            Self::Unsigned(v) => Ok(i128::from(v)),
            Self::Float(v) => {
                // Exclusive 2^64 upper bound, not u64::MAX as f64 (which rounds
                // up to 2^64). i128 keeps both signed and unsigned bounds exact.
                // This rejects infinities/NaNs before any float-to-integer cast.
                if !(-9_223_372_036_854_775_808.0..18_446_744_073_709_551_616.0).contains(&v) {
                    return Err(range_error());
                }
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "The finite range is checked above; the roundtrip below rejects fractional loss."
                )]
                let integer = v as i128;
                if integer as f64 != v {
                    return Err(range_error());
                }
                Ok(integer)
            }
        }
    }
    pub(crate) fn ordinal(self) -> Result<u16> {
        if matches!(self, Self::Float(_)) {
            return Err(mismatch());
        }
        u16::try_from(self.integer()?).map_err(|_| range_error())
    }
    pub(crate) fn convert(self, target: T) -> Result<Reader<'static>> {
        macro_rules! integer {
            ($ty:ty, $variant:ident) => {
                Reader::$variant(<$ty>::try_from(self.integer()?).map_err(|_| range_error())?)
            };
        }
        Ok(match target {
            T::Int8 => integer!(i8, Int8),
            T::Int16 => integer!(i16, Int16),
            T::Int32 => integer!(i32, Int32),
            T::Int64 => integer!(i64, Int64),
            T::UInt8 => integer!(u8, UInt8),
            T::UInt16 => integer!(u16, UInt16),
            T::UInt32 => integer!(u32, UInt32),
            T::UInt64 => integer!(u64, UInt64),
            // Cast integers directly to the requested float width: going via
            // f64 can double-round integers near an f32 rounding midpoint.
            T::Float32 => Reader::Float32(match self {
                Self::Signed(v) => v as f32,
                Self::Unsigned(v) => v as f32,
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "Float32 conversion intentionally rounds and overflows to infinity, matching C++."
                )]
                Self::Float(v) => v as f32,
            }),
            T::Float64 => Reader::Float64(match self {
                Self::Signed(v) => v as f64,
                Self::Unsigned(v) => v as f64,
                Self::Float(v) => v,
            }),
            _ => return Err(mismatch()),
        })
    }
}

pub(super) fn convert(value: Reader<'_>, target: Type) -> Result<Reader<'_>> {
    let ty = target.which();
    if let Some(number) = Number::from_reader(&value) {
        return if let T::Enum(schema) = ty {
            Ok(Reader::Enum(Enum::new(number.ordinal()?, schema.into())))
        } else {
            number.convert(ty)
        };
    }
    Ok(match (value, ty) {
        (Reader::Void, T::Void) => Reader::Void,
        (Reader::Bool(v), T::Bool) => Reader::Bool(v),
        (Reader::Text(v), T::Text) => Reader::Text(v),
        (Reader::Data(v), T::Data) => Reader::Data(v),
        (Reader::Text(v), T::Data) => Reader::Data(v.as_bytes()),
        (Reader::Text(v), T::Enum(schema)) => {
            let schema: crate::schema::EnumSchema = schema.into();
            let mut ordinal = None;
            for member in schema.get_enumerants()? {
                if member.get_proto().get_name()?.as_bytes() == v.as_bytes() {
                    ordinal = Some(member.get_ordinal());
                    break;
                }
            }
            Reader::Enum(Enum::new(ordinal.ok_or_else(mismatch)?, schema))
        }
        (Reader::Enum(v), T::Enum(schema))
            if v.get_schema().get_proto().get_id()
                == crate::schema::EnumSchema::from(schema).get_proto().get_id() =>
        {
            Reader::Enum(v)
        }
        (Reader::Struct(v), T::Struct(_)) if v.get_schema().as_type().equals(target)? => {
            Reader::Struct(v)
        }
        (Reader::List(v), T::List(element)) if v.element_type().equals(element)? => Reader::List(v),
        (Reader::AnyPointer(v), T::AnyPointer) => Reader::AnyPointer(v),
        (Reader::Capability(v), T::Capability) => Reader::Capability(v),
        (Reader::Capability(v), T::Interface(schema)) => {
            Reader::Capability(v.upcast(schema.into())?)
        }
        _ => return Err(mismatch()),
    })
}
