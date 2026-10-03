//! Untyped field access with schema constraints retained by the returned view.
use super::*;

impl<'a, 's: 'a> Builder<'a, 's> {
    fn any_slot(&self, name: &str, kind: PointerKind) -> Result<(Field<'s>, usize)> {
        let f = self.schema.field(name)?;
        require(
            f.get_type()? == Type::AnyPointer(kind),
            "pointer constraint mismatch",
        )?;
        let field::Slot(slot) = f.get_proto().which()? else {
            return Err(invalid("not a slot"));
        };
        Ok((f, slot.get_offset() as usize))
    }

    /// Borrow an active, unconstrained AnyPointer field without changing it.
    /// Wire interpretation is deferred to the returned raw pointer builder.
    /// For constrained fields use `get_any_struct()` or `get_any_list()`;
    /// capability/interface fields are not raw pointer accessors.
    /// The pointer continues to borrow the message:
    ///
    /// ```compile_fail
    /// use capnp::{any_pointer, schema_loader::dynamic::Builder};
    /// fn escape<'a, 's: 'a>(root: Builder<'a, 's>) -> any_pointer::Builder<'static> {
    ///     root.get_any_pointer("any").unwrap()
    /// }
    /// ```
    pub fn get_any_pointer(self, name: &str) -> Result<any_pointer::Builder<'a>> {
        let (f, offset) = self.any_slot(name, PointerKind::Any)?;
        self.as_reader().check_active(&f)?;
        Ok(any_pointer::Builder::new(
            self.builder.get_pointer_field(offset),
        ))
    }

    /// Borrow an active AnyStruct field, preserving its physical sections and
    /// descendants. A null pointer materializes an empty struct. Incompatible
    /// storage is rejected; the returned view cannot replace the outer pointer
    /// with a list or capability.
    /// Parent and child views remain exclusively borrowed:
    ///
    /// ```compile_fail
    /// use capnp::schema_loader::dynamic::Builder;
    /// fn alias(mut root: Builder<'_, '_>) {
    ///     let mut child = root.reborrow().get_any_struct("structure").unwrap();
    ///     root.clear_named("structure").unwrap();
    ///     child.get_data_section()[0] = 1;
    /// }
    /// ```
    pub fn get_any_struct(self, name: &str) -> Result<crate::any_struct::Builder<'a>> {
        let (f, offset) = self.any_slot(name, PointerKind::Struct)?;
        self.as_reader().check_active(&f)?;
        any_pointer::Builder::new(self.builder.get_pointer_field(offset)).get_as()
    }

    /// Borrow an active AnyList field with its actual element encoding and
    /// layout. Null pointers yield empty void-list views without allocating.
    /// Incompatible pointers fail without replacement or union selection.
    pub fn get_any_list(self, name: &str) -> Result<crate::any_list::Builder<'a>> {
        let (f, offset) = self.any_slot(name, PointerKind::List)?;
        self.as_reader().check_active(&f)?;
        any_pointer::Builder::new(self.builder.get_pointer_field(offset)).get_as()
    }

    /// Select and clear an unconstrained AnyPointer field, releasing its old
    /// descendants and capability owners. The returned pointer is null.
    /// A schema-constraint mismatch leaves both selection and storage unchanged.
    pub fn init_any_pointer(mut self, name: &str) -> Result<any_pointer::Builder<'a>> {
        let (f, offset) = self.any_slot(name, PointerKind::Any)?;
        self.activate(&f)?;
        let mut pointer = any_pointer::Builder::new(self.builder.get_pointer_field(offset));
        pointer.clear();
        Ok(pointer)
    }

    /// Select and replace an AnyStruct field with zeroed sections of the given
    /// size. Data is measured in words (eight bytes), pointers in slots.
    pub fn init_any_struct(
        mut self,
        name: &str,
        data_words: u16,
        pointer_count: u16,
    ) -> Result<crate::any_struct::Builder<'a>> {
        let (f, offset) = self.any_slot(name, PointerKind::Struct)?;
        self.activate(&f)?;
        Ok(
            any_pointer::Builder::new(self.builder.get_pointer_field(offset))
                .init_as_any_struct(data_words, pointer_count),
        )
    }

    /// Select and replace an AnyList field with a zeroed non-struct list.
    /// Encoding/count errors are rejected before changing the union or storage.
    /// Use `init_any_struct_list()` for inline-composite elements.
    pub fn init_any_list(
        mut self,
        name: &str,
        element_size: crate::any_list::ElementSize,
        count: u32,
    ) -> Result<crate::any_list::Builder<'a>> {
        let (f, offset) = self.any_slot(name, PointerKind::List)?;
        require(
            element_size != layout::ElementSize::InlineComposite,
            "use init_any_struct_list for struct elements",
        )?;
        require(count < 1 << 29, "list element count exceeds wire limit")?;
        self.activate(&f)?;
        any_pointer::Builder::new(self.builder.get_pointer_field(offset))
            .init_as_any_list(element_size, count)
    }

    /// Select and replace an AnyList field with zeroed inline structs. Both
    /// the element count and total word count are checked before replacement.
    pub fn init_any_struct_list(
        mut self,
        name: &str,
        data_words: u16,
        pointer_count: u16,
        count: u32,
    ) -> Result<crate::any_struct_list::Builder<'a>> {
        let (f, offset) = self.any_slot(name, PointerKind::List)?;
        require(
            count < 1 << 30,
            "struct list element count exceeds wire limit",
        )?;
        require(
            u64::from(count) * (u64::from(data_words) + u64::from(pointer_count)) < 1 << 29,
            "struct list exceeds wire limit",
        )?;
        self.activate(&f)?;
        any_pointer::Builder::new(self.builder.get_pointer_field(offset))
            .init_as_list_of_any_struct(data_words, pointer_count, count)
    }
}
