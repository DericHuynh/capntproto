//! A whole struct root with a loader-borrowed schema and message-scoped owners.
use super::*;

pub struct Root<'message, 'schema: 'message> {
    pub(super) pointer: any_pointer::Builder<'message>,
    schema: Schema<'schema>,
}
impl<'m, 's: 'm> Root<'m, 's> {
    pub fn new(pointer: any_pointer::Builder<'m>, schema: Schema<'s>) -> Result<Self> {
        schema.struct_size()?;
        require(
            !is_group(&Type::Struct(schema.clone()))?,
            "group is not an independent result",
        )?;
        Ok(Self { pointer, schema })
    }
    pub fn with_orphanage(self) -> (Self, Orphanage<'m>) {
        let token = Orphanage::new(self.pointer.builder.identity());
        (self, token)
    }
    pub fn schema(&self) -> Schema<'s> {
        self.schema.clone()
    }
    pub fn get(&mut self) -> Result<Builder<'_, 's>> {
        Builder::new(self.pointer.reborrow(), self.schema.clone())
    }
    pub fn as_reader(&self) -> Result<Reader<'_, 's>> {
        Reader::new(
            any_pointer::Reader::new(self.pointer.builder.as_reader()),
            self.schema.clone(),
        )
    }
    pub fn is_null(&self) -> bool {
        self.pointer.is_null()
    }
    pub fn clear(&mut self) {
        self.pointer.clear();
    }
    /// Failure returns the owner without modifying either root or capabilities.
    pub fn adopt<'a>(
        &mut self,
        mut orphan: Orphan<'a, 's>,
    ) -> core::result::Result<(), AdoptError<Orphan<'a, 's>>> {
        let result = (|| {
            orphan.check(
                &Type::Struct(self.schema.clone()),
                self.pointer.builder.identity(),
            )?;
            let OwnedValue::Pointer(object) = &mut orphan.data.value else {
                return Err(mismatch());
            };
            self.pointer.builder.adopt(object)
        })();
        result.map_err(|error| AdoptError { error, orphan })
    }
    pub fn disown<'a>(&mut self, token: &Orphanage<'a>) -> Result<Orphan<'a, 's>> {
        token.0.check(self.pointer.builder.identity())?;
        self.as_reader()?;
        Ok(Orphan::new(
            Type::Struct(self.schema.clone()),
            token,
            OwnedValue::Pointer(self.pointer.builder.disown()?),
        ))
    }
}
