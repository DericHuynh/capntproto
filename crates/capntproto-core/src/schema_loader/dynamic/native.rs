//! Native enum conversions and transfers of existing RPC hooks. Hook transfers
//! do not resolve promises or change the authority represented by those hooks.
use super::*;

impl<'a, 's: 'a> Value<'a, 's> {
    /// Cast an enum value to a generated closed or open field-API enum. This
    /// checks the enum ID, erasing brands and loader identity without requiring
    /// native registration, as C++ `DynamicEnum::as<T>()` does. Unknown ordinals
    /// survive in open enums; closed Rust enums return `NotInSchema`. Other
    /// value kinds are rejected without numeric coercion.
    pub fn cast_enum<E: crate::introspect::Introspect + TryFrom<u16>>(&self) -> Result<E> {
        let Self::Enum(value, schema) = self else {
            return Err(Error::from_kind(crate::ErrorKind::TypeMismatch));
        };
        if schema.kind() != Kind::Enum {
            return Err(Error::from_kind(crate::ErrorKind::TypeMismatch));
        }
        crate::dynamic_value::cast_enum(schema.id(), *value)
    }
    /// Look up this enum value's ordinal in its loaded schema. Unknown ordinals
    /// return `None`; non-enum values return an error. The result retains its
    /// declaring schema and brand and borrows the loader, not the message.
    ///
    /// ```compile_fail
    /// use capnp::schema_loader::{SchemaLoader, dynamic::Value};
    /// fn escape(loader: SchemaLoader, id: u64) {
    ///     let member = Value::Enum(0, loader.get(id).unwrap())
    ///         .get_enumerant().unwrap().unwrap();
    ///     drop(loader);
    ///     let _ = member.index(); // The declaration still borrows this loader.
    /// }
    /// ```
    pub fn get_enumerant(&self) -> Result<Option<super::super::Enumerant<'s>>> {
        let Self::Enum(value, schema) = self else {
            return Err(Error::from_kind(crate::ErrorKind::TypeMismatch));
        };
        schema.enumerant_at(*value)
    }
}

impl Client<'_> {
    fn check_native<C: capability::FromClientHook>(&self) -> Result<()> {
        if let Some(target) = C::interface_schema() {
            let source = self
                .schema
                .as_ref()
                .ok_or_else(|| invalid("capability has no interface metadata"))?;
            Type::Interface(source.clone()).require_usable_as_type(
                crate::introspect::TypeVariant::Interface(target.raw).into(),
            )?;
        }
        self.as_client()?;
        Ok(())
    }
    /// Share this capability as a generated client. The declaring interface must
    /// be registered with the loader and have the same ID. Generic arguments
    /// are erased, as in C++ `DynamicCapability::Client::as<T>()`; inheritance
    /// alone does not make a native cast valid. No promise is polled.
    /// A typeless `capability::Client` target only removes metadata.
    pub fn cast_native<C: capability::FromClientHook>(&self) -> Result<C> {
        self.check_native::<C>()?;
        Ok(C::new(self.as_client()?.hook.add_ref()))
    }
    /// Transfer the existing hook without cloning it. On failure the error and
    /// original dynamic client are returned, preserving ownership for retry.
    /// The native client no longer borrows the schema loader.
    ///
    /// ```compile_fail
    /// fn duplicate(client: capnp::schema_loader::dynamic::Client<'_>) {
    ///     let moved = client.release_native::<capnp::capability::Client>();
    ///     let _ = client.as_client(); // The original owner was consumed.
    /// }
    /// ```
    pub fn release_native<C: capability::FromClientHook>(
        self,
    ) -> core::result::Result<C, (Error, Self)> {
        if let Err(error) = self.check_native::<C>() {
            return Err((error, self));
        }
        Ok(C::new(self.client.unwrap().hook))
    }
}

impl Pipeline<'_> {
    /// Transfer a dynamic struct pipeline to its generated native pipeline.
    /// Registration and the struct ID must match; generic arguments are erased.
    /// Pending calls and pointer paths are preserved without fetching results or
    /// projecting capabilities. Failure returns the original pipeline for retry.
    #[allow(
        clippy::result_large_err,
        reason = "Return the original pipeline without allocating on a failed conversion."
    )]
    pub fn release_native<T: crate::traits::OwnedStruct + crate::traits::Pipelined>(
        self,
    ) -> core::result::Result<T::Pipeline, (Error, Self)>
    where
        T::Pipeline: capability::FromTypelessPipeline,
    {
        if let Err(error) = self.schema.check_native::<T>() {
            return Err((error, self));
        }
        Ok(capability::FromTypelessPipeline::new(self.pipeline))
    }
}
