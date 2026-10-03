//! Attach or erase runtime metadata without replacing storage.
use super::*;

impl<'a, 's: 'a> Reader<'a, 's> {
    pub(crate) fn from_struct(
        reader: layout::StructReader<'a>,
        schema: Schema<'s>,
    ) -> Result<Self> {
        schema.struct_size()?;
        Ok(Self { reader, schema })
    }
}
impl<'a, 's: 'a> Builder<'a, 's> {
    /// Erase the schema and retain the existing mutable struct storage.
    /// Unknown fields and capability tables are preserved without copying,
    /// traversing children or requiring native registration. For a group, this
    /// exposes the containing struct, including siblings outside the group.
    /// The original storage borrow remains in force; use `reborrow()` to keep
    /// using the loaded builder after the erased view is released.
    ///
    /// ```compile_fail
    /// use capnp::schema_loader::dynamic::Builder;
    /// fn alias(mut value: Builder<'_, '_>) {
    ///     let mut raw = value.reborrow().into_any_struct();
    ///     value.clear_named("number").unwrap();
    ///     raw.get_data_section()[0] = 1;
    /// }
    /// ```
    pub fn into_any_struct(self) -> crate::any_struct::Builder<'a> {
        crate::any_struct::Builder::new(self.builder)
    }

    // The caller has checked that both physical sections fit this schema.
    pub(crate) fn from_checked_struct(
        builder: layout::StructBuilder<'a>,
        schema: Schema<'s>,
    ) -> Self {
        Self { builder, schema }
    }
}
fn check_element_type(element: &Type<'_>) -> Result<()> {
    // Iterative so deeply nested list metadata cannot overflow the call stack.
    let mut ty = element;
    while let Type::List(inner) = ty {
        ty = inner;
    }
    match ty {
        Type::Unknown(_) | Type::Parameter(..) => {
            Err(invalid("list cast requires a resolved element type"))
        }
        Type::Struct(schema) => schema.struct_size().map(|_| ()),
        Type::Enum(schema) => require(schema.kind() == Kind::Enum, "not an enum schema"),
        Type::Interface(schema) => {
            require(schema.kind() == Kind::Interface, "not an interface schema")
        }
        _ => Ok(()),
    }
}
impl<'a, 's: 'a> ListReader<'a, 's> {
    pub(crate) fn from_list(reader: layout::ListReader<'a>, element: Type<'s>) -> Result<Self> {
        check_element_type(&element)?;
        reader.check_element_size(element.element_size())?;
        if matches!(element, Type::Struct(_)) {
            reader.check_struct_nesting()?;
        }
        Ok(Self { reader, element })
    }
}
impl<'a, 's: 'a> ListBuilder<'a, 's> {
    /// Erase the element type while retaining the physical list layout and
    /// capability table. Inline-composite projections expose all original
    /// fields, not just the field selected by the former element type.
    /// This neither copies storage nor accesses children. The original storage
    /// borrow remains in force; use `reborrow()` for a temporary erased view.
    ///
    /// ```compile_fail
    /// use capnp::{any_list, schema_loader::dynamic::ListBuilder};
    /// fn escape(value: ListBuilder<'_, '_>) -> any_list::Builder<'static> {
    ///     value.into_any_list()
    /// }
    /// ```
    ///
    /// ```compile_fail
    /// use capnp::schema_loader::dynamic::{ListBuilder, Value};
    /// fn alias(mut value: ListBuilder<'_, '_>) {
    ///     let raw = value.reborrow().into_any_list();
    ///     value.set(0, Value::UInt32(1)).unwrap();
    ///     let _ = raw.len();
    /// }
    /// ```
    pub fn into_any_list(self) -> crate::any_list::Builder<'a> {
        crate::any_list::Builder::new(self.builder)
    }

    pub(crate) fn from_list(builder: layout::ListBuilder<'a>, element: Type<'s>) -> Result<Self> {
        check_element_type(&element)?;
        builder
            .as_reader()
            .check_element_size(element.element_size())?;
        if let Type::Struct(schema) = &element {
            builder.check_struct_size(schema.struct_size()?)?;
        }
        Ok(Self { builder, element })
    }
}
