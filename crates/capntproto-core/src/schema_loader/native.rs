//! Compatibility with registered native types. Native casts erase brands;
//! assignment and orphan transfer continue to use their stricter contracts.
use super::*;

impl Type<'_> {
    /// Require this loaded type to be usable as a native type. Named schemas
    /// require registration with their owning loader and the same kind/ID.
    /// Primitive kinds and list nesting must agree. Generic arguments are
    /// erased, matching C++ `Type::requireUsableAs`, rather than assignment.
    /// AnyPointer constraints and symbolic parameters share the AnyPointer wire
    /// type. This check reads metadata only and does not extract capabilities.
    pub fn require_usable_as<T: crate::introspect::Introspect>(&self) -> Result<()> {
        self.require_usable_as_type(T::introspect())
    }
    pub(super) fn require_usable_as_type(&self, mut native: crate::introspect::Type) -> Result<()> {
        use crate::introspect::TypeVariant as N;
        let mut loaded = self;
        // List nesting is represented by a count on the compiled side. Walk
        // the loaded chain iteratively, without recursive comparison/callbacks.
        loop {
            match (loaded, native.which()) {
                (Type::List(inner), N::List(element)) => {
                    loaded = inner;
                    native = element;
                }
                (Type::List(_), _) | (_, N::List(_)) => {
                    return Err(invalid("native list nesting mismatch"))
                }
                _ => break,
            }
        }
        let (schema, id, kind) = match (loaded, native.which()) {
            (Type::Void, N::Void)
            | (Type::Bool, N::Bool)
            | (Type::Int8, N::Int8)
            | (Type::Int16, N::Int16)
            | (Type::Int32, N::Int32)
            | (Type::Int64, N::Int64)
            | (Type::UInt8, N::UInt8)
            | (Type::UInt16, N::UInt16)
            | (Type::UInt32, N::UInt32)
            | (Type::UInt64, N::UInt64)
            | (Type::Float32, N::Float32)
            | (Type::Float64, N::Float64)
            | (Type::Text, N::Text)
            | (Type::Data, N::Data)
            | (Type::AnyPointer(_) | Type::Parameter(..), N::AnyPointer | N::Capability) => {
                return Ok(())
            }
            (Type::Struct(s), N::Struct(raw)) => (
                s,
                crate::schema::StructSchema::new(raw).get_proto().get_id(),
                Kind::Struct,
            ),
            (Type::Enum(s), N::Enum(raw)) => (
                s,
                crate::schema::EnumSchema::new(raw).get_proto().get_id(),
                Kind::Enum,
            ),
            (Type::Interface(s), N::Interface(raw)) => (
                s,
                crate::schema::InterfaceSchema::new(raw)
                    .get_proto()
                    .get_id(),
                Kind::Interface,
            ),
            _ => return Err(invalid("native type kind mismatch")),
        };
        require(
            schema.kind() == kind
                && schema.loader.native_ids.contains(&schema.id)
                && schema.id == id,
            "native schema was not registered or has a different ID",
        )
    }
}
