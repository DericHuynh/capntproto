//! Capability reflection over the ordinary RPC hooks. Schema metadata describes
//! an interface; it does not grant authority or validate the remote implementation.
#[cfg(feature = "alloc")]
use crate::{
    any_pointer, capability, dynamic_struct,
    schema::{Field, Method, StructSchema},
};
use crate::{
    introspect::TypeVariant, private::layout::PointerReader, schema::InterfaceSchema, Error,
    ErrorKind, Result,
};

/// An owned capability and its optional compiled interface metadata. Old bindings
/// expose unbranded capabilities; new bindings retain their interface schema.
#[derive(Clone)]
pub struct Client {
    #[cfg(feature = "alloc")]
    client: Option<capability::Client>,
    schema: Option<InterfaceSchema>,
}
impl Client {
    /// Describe the underlying capability without calling or resolving it.
    /// For debugging only; the returned text is not a stable format.
    #[cfg(feature = "alloc")]
    pub fn debug_info(&self) -> alloc::string::String {
        self.client
            .as_ref()
            .map_or_else(|| "null".into(), capability::Client::debug_info)
    }

    pub fn null(schema: Option<InterfaceSchema>) -> Self {
        Self {
            #[cfg(feature = "alloc")]
            client: None,
            schema,
        }
    }
    pub fn get_schema(&self) -> Option<InterfaceSchema> {
        self.schema
    }
    pub(crate) fn from_pointer(
        pointer: PointerReader<'_>,
        ty: crate::introspect::Type,
    ) -> Result<Self> {
        let schema = match ty.which() {
            TypeVariant::Interface(raw) => Some(raw.into()),
            _ => None,
        };
        #[cfg(feature = "alloc")]
        {
            Ok(Self {
                client: if pointer.is_null() {
                    None
                } else {
                    Some(capability::Client::new(pointer.get_capability()?))
                },
                schema,
            })
        }
        #[cfg(not(feature = "alloc"))]
        {
            if !pointer.is_null() {
                return Err(Error::from_kind(
                    ErrorKind::MessageContainsInvalidCapabilityPointer,
                ));
            }
            Ok(Self::null(schema))
        }
    }
    pub fn upcast(&self, schema: InterfaceSchema) -> Result<Self> {
        self.validate_for(Some(schema))?;
        let mut result = self.clone();
        result.schema = Some(schema);
        Ok(result)
    }
    pub(crate) fn validate_for(&self, target: Option<InterfaceSchema>) -> Result<()> {
        if let (Some(source), Some(target)) = (self.schema, target) {
            if !source.extends(target)? {
                return Err(Error::from_kind(ErrorKind::TypeMismatch));
            }
        }
        Ok(())
    }
    pub(crate) fn set_pointer(
        self,
        mut pointer: crate::private::layout::PointerBuilder<'_>,
        ty: crate::introspect::Type,
    ) -> Result<()> {
        self.validate_for(match ty.which() {
            TypeVariant::Interface(raw) => Some(raw.into()),
            _ => None,
        })?;
        #[cfg(feature = "alloc")]
        {
            if let Some(client) = self.client {
                pointer.set_capability(client.hook);
            } else {
                pointer.clear();
            }
            Ok(())
        }
        #[cfg(not(feature = "alloc"))]
        {
            pointer.clear();
            Ok(())
        }
    }
}

#[cfg(feature = "alloc")]
mod allocated {
    use super::*;
    use crate::{
        capability::{CallHints, FromClientHook, Promise},
        private::capability::ResponseHook,
    };
    use alloc::{boxed::Box, rc::Rc};
    impl Client {
        pub fn new<C: FromClientHook>(client: C, schema: InterfaceSchema) -> Self {
            Self {
                client: Some(capability::Client::new(client.into_client_hook())),
                schema: Some(schema),
            }
        }
        /// Recover a generated client, checking its interface when metadata is available.
        pub fn cast<C: FromClientHook>(&self) -> Result<C> {
            self.validate_for(C::interface_schema())?;
            Ok(C::new(self.as_client()?.hook.add_ref()))
        }
        /// Native conversion matching C++ `DynamicCapability::Client::as<T>()`.
        /// It checks the declaring interface but erases generic arguments. Use
        /// cast() for a brand-checked conversion, or upcast() before converting
        /// an inherited interface to its generated client.
        pub fn cast_native<C: FromClientHook>(&self) -> Result<C> {
            self.check_native::<C>()?;
            Ok(C::new(self.as_client()?.hook.add_ref()))
        }
        fn check_native<C: FromClientHook>(&self) -> Result<()> {
            if let (Some(source), Some(target)) = (self.schema, C::interface_schema()) {
                if source.get_proto().get_id() != target.get_proto().get_id() {
                    return Err(Error::from_kind(ErrorKind::TypeMismatch));
                }
            }
            self.as_client()?;
            Ok(())
        }
        /// Transfer the hook using the same native type check as `cast_native()`.
        /// This neither clones nor resolves the capability. A rejected conversion
        /// returns the original client with its error so ownership can be recovered.
        #[allow(
            clippy::result_large_err,
            reason = "Return the original owner without allocating on a failed conversion."
        )]
        pub fn release_native<C: FromClientHook>(self) -> core::result::Result<C, (Error, Self)> {
            if let Err(error) = self.check_native::<C>() {
                return Err((error, self));
            }
            Ok(C::new(self.client.unwrap().hook))
        }
        pub fn as_client(&self) -> Result<&capability::Client> {
            self.client
                .as_ref()
                .ok_or_else(|| Error::failed("null dynamic capability".into()))
        }
        pub fn new_request(
            &self,
            name: &str,
            size_hint: Option<crate::MessageSize>,
        ) -> Result<Request> {
            let schema = self
                .schema
                .ok_or_else(|| Error::failed("capability has no interface metadata".into()))?;
            self.new_request_for(schema.get_method_by_name(name)?, size_hint)
        }
        pub fn new_request_for(
            &self,
            method: Method,
            size_hint: Option<crate::MessageSize>,
        ) -> Result<Request> {
            if let Some(schema) = self.schema {
                if !schema.extends(method.get_containing_interface())? {
                    return Err(Error::from_kind(ErrorKind::TypeMismatch));
                }
            }
            let request = self.as_client()?.hook.new_call_with_hints(
                method.get_containing_interface().get_proto().get_id(),
                method.get_index(),
                size_hint,
                CallHints {
                    no_promise_pipelining: method.no_promise_pipelining(),
                    only_promise_pipeline: false,
                },
            );
            Ok(Request {
                hook: request.hook,
                method,
            })
        }
        pub fn when_resolved(&self) -> Promise<(), Error> {
            match self.as_client() {
                Ok(client) => client.when_resolved(),
                Err(error) => Promise::err(error),
            }
        }
    }
    impl From<capability::Client> for Client {
        fn from(client: capability::Client) -> Self {
            Self {
                client: Some(client),
                schema: None,
            }
        }
    }
    pub struct Request {
        pub(crate) hook: Box<dyn crate::private::capability::RequestHook>,
        method: Method,
    }
    impl Request {
        pub fn get(&mut self) -> Result<dynamic_struct::Builder<'_>> {
            let schema = self.method.get_param_type();
            Ok(dynamic_struct::Builder::new(
                self.hook
                    .get()
                    .builder
                    .get_struct(dynamic_struct::struct_size_from_schema(schema)?, None)?,
                schema,
            ))
        }
        pub fn send(self) -> RemotePromise {
            let result = self.hook.send();
            let schema = self.method.get_result_type();
            RemotePromise {
                promise: Promise::from_future(async move {
                    Ok(Response {
                        hook: result.promise.await?.hook,
                        schema,
                    })
                }),
                pipeline: Pipeline {
                    pipeline: result.pipeline,
                    schema,
                },
            }
        }
        pub fn send_for_pipeline(self) -> Pipeline {
            Pipeline {
                pipeline: self.hook.send_for_pipeline(),
                schema: self.method.get_result_type(),
            }
        }
        pub fn send_streaming(self) -> Result<Promise<(), Error>> {
            if !self.method.is_streaming() {
                return Err(Error::from_kind(ErrorKind::TypeMismatch));
            }
            Ok(self.hook.send_streaming())
        }
        pub fn send_ignoring_result(self) -> Promise<(), Error> {
            capability::Request::<any_pointer::Owned, any_pointer::Owned>::new(self.hook)
                .send_ignoring_result()
        }
    }
    pub struct Response {
        hook: Box<dyn ResponseHook>,
        schema: Option<StructSchema>,
    }
    impl Response {
        pub fn get(&self) -> Result<dynamic_struct::Reader<'_>> {
            let schema = self
                .schema
                .ok_or_else(|| Error::failed("streaming response has no data".into()))?;
            Ok(dynamic_struct::Reader::new(
                self.hook.get()?.reader.get_struct(None)?,
                schema,
            ))
        }
    }
    pub struct RemotePromise {
        pub promise: Promise<Response, Error>,
        pub pipeline: Pipeline,
    }
    pub struct Pipeline {
        pipeline: any_pointer::Pipeline,
        schema: Option<StructSchema>,
    }
    impl Clone for Pipeline {
        fn clone(&self) -> Self {
            Self {
                pipeline: self.pipeline.noop(),
                schema: self.schema,
            }
        }
    }
    pub enum PipelineValue {
        Struct(Pipeline),
        Capability(Client),
    }
    impl Pipeline {
        /// Attach compiled result metadata to an independently built pipeline.
        pub fn new(pipeline: any_pointer::Pipeline, schema: StructSchema) -> Self {
            Self {
                pipeline,
                schema: Some(schema),
            }
        }
        /// Transfer the existing pipeline to a generated struct pipeline, erasing
        /// generic arguments as in C++ `DynamicStruct::Pipeline::releaseAs<T>()`.
        /// No result is awaited or capability projected. Failure returns the error
        /// and original pipeline. Streaming responses have no struct pipeline.
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
            if !self
                .schema
                .is_some_and(|s| s.as_type().loose_equals(T::introspect()))
            {
                return Err((Error::from_kind(ErrorKind::TypeMismatch), self));
            }
            Ok(capability::FromTypelessPipeline::new(self.pipeline))
        }
        pub fn get_by_name(&self, name: &str) -> Result<PipelineValue> {
            let schema = self
                .schema
                .ok_or_else(|| Error::failed("streaming response has no pipeline".into()))?;
            self.get(schema.get_field_by_name(name)?)
        }
        pub fn get(&self, field: Field) -> Result<PipelineValue> {
            let schema = self
                .schema
                .ok_or_else(|| Error::from_kind(ErrorKind::TypeMismatch))?;
            if !schema.equals(field.parent)?
                || field.get_proto().get_discriminant_value()
                    != crate::schema_capnp::field::NO_DISCRIMINANT
            {
                return Err(Error::from_kind(ErrorKind::TypeMismatch));
            }
            let pipeline = match field.get_proto().which()? {
                crate::schema_capnp::field::Group(_) => self.pipeline.noop(),
                crate::schema_capnp::field::Slot(slot) => self.pipeline.get_pointer_field(
                    slot.get_offset()
                        .try_into()
                        .map_err(|_| Error::from_kind(ErrorKind::TypeMismatch))?,
                ),
            };
            match field.get_type().which() {
                TypeVariant::Struct(_) => Ok(PipelineValue::Struct(Self {
                    pipeline,
                    schema: Some(field.get_type().as_struct_schema()?),
                })),
                TypeVariant::Interface(raw) => Ok(PipelineValue::Capability(Client::new(
                    capability::Client::new(pipeline.as_cap()),
                    raw.into(),
                ))),
                TypeVariant::Capability => Ok(PipelineValue::Capability(
                    capability::Client::new(pipeline.as_cap()).into(),
                )),
                _ => Err(Error::from_kind(ErrorKind::TypeMismatch)),
            }
        }
    }
    /// Dynamic servers use an explicit object-wide cancellation policy, matching
    /// C++ DynamicCapability::Server; schema allowCancellation annotations do not apply.
    pub trait Server: 'static {
        fn get_schema(&self) -> InterfaceSchema;
        fn allow_cancellation(&self) -> bool {
            false
        }
        fn get_hooks(&self) -> Option<&dyn capability::ServerHooks> {
            None
        }
        fn call(self: Rc<Self>, method: Method, context: CallContext) -> Promise<(), Error>;
    }
    pub struct CallContext {
        params: Option<capability::Params<any_pointer::Owned>>,
        results: capability::Results<any_pointer::Owned>,
        method: Method,
    }
    impl CallContext {
        pub fn get_params(&self) -> Result<dynamic_struct::Reader<'_>> {
            let params = self
                .params
                .as_ref()
                .ok_or_else(|| Error::failed("parameters already released".into()))?;
            Ok(dynamic_struct::Reader::new(
                params.hook.get()?.reader.get_struct(None)?,
                self.method.get_param_type(),
            ))
        }
        pub fn release_params(&mut self) {
            self.params.take();
        }
        pub fn get_results(&mut self) -> Result<dynamic_struct::Builder<'_>> {
            self.get_results_with_size_hint(None)
        }
        pub fn get_results_with_size_hint(
            &mut self,
            size_hint: Option<crate::MessageSize>,
        ) -> Result<dynamic_struct::Builder<'_>> {
            let schema = self
                .method
                .get_result_type()
                .ok_or_else(|| Error::failed("streaming method has no results".into()))?;
            Ok(dynamic_struct::Builder::new(
                self.results
                    .hook
                    .get_with_size_hint(size_hint)?
                    .builder
                    .get_struct(dynamic_struct::struct_size_from_schema(schema)?, None)?,
                schema,
            ))
        }
        pub fn init_results(
            &mut self,
            size_hint: Option<crate::MessageSize>,
        ) -> Result<dynamic_struct::Builder<'_>> {
            let schema = self
                .method
                .get_result_type()
                .ok_or_else(|| Error::from_kind(ErrorKind::TypeMismatch))?;
            let pointer = self.results.hook.get_with_size_hint(size_hint)?;
            Ok(dynamic_struct::Builder::new(
                pointer
                    .builder
                    .init_struct(dynamic_struct::struct_size_from_schema(schema)?),
                schema,
            ))
        }
        /// Scoped whole-result adoption, with this method's exact result type.
        pub fn get_results_orphanage(
            &mut self,
            size_hint: Option<crate::MessageSize>,
        ) -> Result<(
            crate::dynamic_orphan::Root<'_>,
            crate::dynamic_orphan::Orphanage<'_>,
        )> {
            let schema = self
                .method
                .get_result_type()
                .ok_or_else(|| Error::from_kind(ErrorKind::TypeMismatch))?;
            Ok(crate::dynamic_orphan::Root::new(
                self.results.hook.get_with_size_hint(size_hint)?,
                schema.as_type(),
            )?
            .with_orphanage())
        }
        pub fn set_results(&mut self, value: dynamic_struct::Reader<'_>) -> Result<()> {
            let schema = self
                .method
                .get_result_type()
                .ok_or_else(|| Error::from_kind(ErrorKind::TypeMismatch))?;
            if !schema.equals(value.get_schema())? {
                return Err(Error::from_kind(ErrorKind::TypeMismatch));
            }
            self.results.hook.get()?.set_as(value)
        }
        pub fn set_pipeline(&mut self) -> Result<()> {
            self.results.set_pipeline()
        }
        /// Publish an independent pipeline with this method's exact result schema.
        /// Eventual result capabilities must resolve to the same hook identities.
        pub fn set_pipeline_from(&mut self, pipeline: Pipeline) -> Result<()> {
            let expected = self
                .method
                .get_result_type()
                .ok_or_else(|| Error::from_kind(ErrorKind::TypeMismatch))?;
            let actual = pipeline
                .schema
                .ok_or_else(|| Error::from_kind(ErrorKind::TypeMismatch))?;
            if !expected.equals(actual)? {
                return Err(Error::from_kind(ErrorKind::TypeMismatch));
            }
            self.results
                .hook
                .set_pipeline_from(pipeline.pipeline.into_hook())
        }
        pub fn tail_call(self, request: Request) -> Promise<(), Error> {
            self.results.hook.tail_call(request.hook)
        }
    }
    pub struct ServerDispatch<S: Server> {
        pub server: Rc<S>,
        pub schema: InterfaceSchema,
    }
    impl<S: Server> Clone for ServerDispatch<S> {
        fn clone(&self) -> Self {
            Self {
                server: self.server.clone(),
                schema: self.schema,
            }
        }
    }
    impl<S: Server> core::ops::Deref for ServerDispatch<S> {
        type Target = S;
        fn deref(&self) -> &S {
            &self.server
        }
    }
    /// Untyped clients let dynamic servers participate in the ordinary server-set,
    /// revocation, file-descriptor and shared-server constructors. Wrap the result
    /// with Client::new() to retain its interface metadata.
    impl<S: Server> capability::FromServer<S> for capability::Client {
        type Dispatch = ServerDispatch<S>;
        fn from_server(server: Rc<S>) -> Self::Dispatch {
            let schema = server.get_schema();
            ServerDispatch { server, schema }
        }
    }
    impl<S: Server> capability::Server for ServerDispatch<S> {
        fn as_ptr(&self) -> usize {
            Rc::as_ptr(&self.server) as usize
        }
        fn get_hooks(&self) -> Option<&dyn capability::ServerHooks> {
            self.server.get_hooks()
        }
        fn dispatch_call(
            self,
            interface_id: u64,
            method_id: u16,
            params: capability::Params<any_pointer::Owned>,
            results: capability::Results<any_pointer::Owned>,
        ) -> capability::DispatchCallResult {
            let method = self
                .schema
                .find_superclass(interface_id)
                .and_then(|schema| {
                    schema.ok_or_else(|| Error::unimplemented("interface not implemented".into()))
                })
                .and_then(|schema| {
                    let methods = schema.get_methods()?;
                    if method_id < methods.len() {
                        Ok(methods.get(method_id))
                    } else {
                        Err(Error::unimplemented("method not implemented".into()))
                    }
                });
            match method {
                Ok(method) => {
                    let allow = self.server.allow_cancellation();
                    capability::DispatchCallResult::with_cancellation_policy(
                        self.server.call(
                            method,
                            CallContext {
                                params: Some(params),
                                results,
                                method,
                            },
                        ),
                        method.is_streaming(),
                        allow,
                    )
                }
                Err(error) => capability::DispatchCallResult::new(Promise::err(error), false),
            }
        }
    }
}
#[cfg(feature = "alloc")]
pub use allocated::*;
