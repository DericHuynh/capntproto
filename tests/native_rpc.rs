use capnp::{
    any_pointer,
    capability::{FromClientHook, IntoTypelessPipeline},
    dynamic_capability as compiled,
    introspect::Introspect,
    schema_loader::{dynamic as loaded, SchemaLoader},
};
use capntproto_test_support::native_rpc_capnp::{derived, factory, other, outer, service, wrong};
use std::{cell::Cell, rc::Rc};
type Text = capnp::text::Owned;
type Data = capnp::data::Owned;
type Outer = outer::Owned<service::Owned<Text>>;
type Changed = outer::Owned<service::Owned<Data>>;
mod native_rpc {
    pub mod integration;
    pub mod verification;
}

fn loader(registered: bool) -> SchemaLoader {
    let mut native = SchemaLoader::default();
    native
        .load_compiled_type_and_dependencies::<factory::Owned>()
        .unwrap();
    native
        .load_compiled_type_and_dependencies::<derived::Owned<Text>>()
        .unwrap();
    if registered {
        return native;
    }
    let mut plain = SchemaLoader::default();
    plain
        .load_batch(native.get_all_loaded().map(|s| s.get_proto()))
        .unwrap();
    plain
}
fn id<T: Introspect>() -> u64 {
    match T::introspect().which() {
        capnp::introspect::TypeVariant::Struct(raw) => {
            capnp::schema::StructSchema::new(raw).get_proto().get_id()
        }
        capnp::introspect::TypeVariant::Interface(raw) => capnp::schema::InterfaceSchema::new(raw)
            .get_proto()
            .get_id(),
        _ => panic!(),
    }
}
fn success<T, O>(result: Result<T, (capnp::Error, O)>) -> T {
    result.unwrap_or_else(|(e, _)| panic!("{e}"))
}
struct Echo {
    alive: Rc<Cell<bool>>,
    calls: Rc<Cell<u64>>,
}
impl Drop for Echo {
    fn drop(&mut self) {
        self.alive.set(false);
    }
}
impl service::Server<Text> for Echo {
    async fn ping(
        self: Rc<Self>,
        params: service::PingParams<Text>,
        mut results: service::PingResults<Text>,
    ) -> capnp::Result<()> {
        self.calls.set(self.calls.get() + 1);
        results.get().set_value(params.get()?.get_value() + 1);
        Ok(())
    }
}
impl derived::Server<Text> for Echo {}

enum Source<'a> {
    CompiledClient(compiled::Client),
    LoadedClient(loaded::Client<'a>),
    CompiledPipeline(compiled::Pipeline),
    LoadedPipeline(loaded::Pipeline<'a>),
}
enum Native {
    Client(service::Client<Text>),
    Pipeline(outer::Pipeline<service::Owned<Text>>),
    Changed(outer::Pipeline<service::Owned<Data>>),
}
fn native_client<C: FromClientHook>(client: C) -> Native {
    Native::Client(service::Client::new(client.into_client_hook()))
}

fn source(
    loader: &SchemaLoader,
    case: u64,
    compiled: bool,
    alive: Rc<Cell<bool>>,
    calls: Rc<Cell<u64>>,
) -> Source<'_> {
    alive.set(true);
    let client: derived::Client<Text> = capnp_rpc::new_client(Echo { alive, calls });
    if case <= 4 {
        let schema = if case == 4 {
            derived::Client::<Text>::schema()
        } else {
            service::Client::<Text>::schema()
        };
        if compiled {
            Source::CompiledClient(compiled::Client::new(client, schema))
        } else {
            Source::LoadedClient(
                loaded::Client::new(client, loader.get(schema.get_proto().get_id()).unwrap())
                    .unwrap(),
            )
        }
    } else {
        let mut builder = capnp_rpc::PipelineBuilder::<Outer>::new();
        builder
            .get()
            .init_inner()
            .init_body()
            .set_cap(service::Client::<Text>::new(client.into_client_hook()))
            .unwrap();
        let pipeline = builder.build().into_typeless_pipeline();
        if compiled {
            Source::CompiledPipeline(compiled::Pipeline::new(
                pipeline,
                Outer::introspect().as_struct_schema().unwrap(),
            ))
        } else {
            Source::LoadedPipeline(
                loaded::Pipeline::new(pipeline, loader.get(id::<Outer>()).unwrap()).unwrap(),
            )
        }
    }
}
fn share(source: &Source<'_>, case: u64) -> capnp::Result<Native> {
    macro_rules! cast {
        ($c:expr) => {
            match case {
                1 => $c.cast_native::<service::Client<Text>>().map(native_client),
                2 => $c.cast_native::<service::Client<Data>>().map(native_client),
                3 => $c.cast_native::<other::Client>().map(native_client),
                4 => $c.cast_native::<service::Client<Text>>().map(native_client),
                _ => panic!(),
            }
        };
    }
    match source {
        Source::CompiledClient(c) => cast!(c),
        Source::LoadedClient(c) => cast!(c),
        _ => panic!(),
    }
}
fn release(source: Source<'_>, case: u64) -> Result<Native, (capnp::Error, Box<Source<'_>>)> {
    macro_rules! client {
        ($c:expr,$variant:ident) => {
            match case {
                1 | 4 => $c
                    .release_native::<service::Client<Text>>()
                    .map(native_client)
                    .map_err(|(e, c)| (e, Box::new(Source::$variant(c)))),
                2 => $c
                    .release_native::<service::Client<Data>>()
                    .map(native_client)
                    .map_err(|(e, c)| (e, Box::new(Source::$variant(c)))),
                3 => $c
                    .release_native::<other::Client>()
                    .map(native_client)
                    .map_err(|(e, c)| (e, Box::new(Source::$variant(c)))),
                _ => panic!(),
            }
        };
    }
    macro_rules! pipeline {
        ($p:expr,$variant:ident) => {
            match case {
                5 => $p
                    .release_native::<Outer>()
                    .map(Native::Pipeline)
                    .map_err(|(e, p)| (e, Box::new(Source::$variant(p)))),
                6 => $p
                    .release_native::<Changed>()
                    .map(Native::Changed)
                    .map_err(|(e, p)| (e, Box::new(Source::$variant(p)))),
                7 => $p
                    .release_native::<wrong::Owned>()
                    .map(|_| panic!("wrong pipeline accepted"))
                    .map_err(|(e, p)| (e, Box::new(Source::$variant(p)))),
                _ => panic!(),
            }
        };
    }
    match source {
        Source::CompiledClient(c) => client!(c, CompiledClient),
        Source::LoadedClient(c) => client!(c, LoadedClient),
        Source::CompiledPipeline(p) => pipeline!(p, CompiledPipeline),
        Source::LoadedPipeline(p) => pipeline!(p, LoadedPipeline),
    }
}
async fn call(native: &Native) -> u32 {
    macro_rules! ping {
        ($c:expr) => {{
            let mut request = $c.ping_request();
            request.get().set_value(41);
            request
                .send()
                .promise
                .await
                .unwrap()
                .get()
                .unwrap()
                .get_value()
        }};
    }
    match native {
        Native::Client(c) => ping!(c),
        Native::Pipeline(p) => ping!(p.get_inner().get_body().get_cap()),
        Native::Changed(p) => ping!(p.get_inner().get_body().get_cap()),
    }
}

#[test]
fn native_conversions_check_registration_ids_and_erased_brands_and_retain_owners() {
    for mode in [false, true] {
        for registered in [false, true] {
            let loader = loader(registered);
            for case in 1..=7 {
                let alive = Rc::new(Cell::new(false));
                let calls = Rc::new(Cell::new(0));
                let source = source(&loader, case, mode, alive.clone(), calls.clone());
                let accepted = (mode || registered) && matches!(case, 1 | 2 | 5 | 6);
                if case <= 4 {
                    let shared = share(&source, case);
                    assert_eq!(shared.is_ok(), accepted);
                    drop(shared);
                    assert!(alive.get());
                }
                match release(source, case) {
                    Ok(native) => {
                        assert!(accepted);
                        assert!(alive.get());
                        assert_eq!(futures::executor::block_on(call(&native)), 42);
                        assert_eq!(calls.get(), 1);
                        drop(native);
                    }
                    Err((_, source)) => {
                        assert!(!accepted);
                        assert!(alive.get());
                        drop(source);
                    }
                }
                assert!(!alive.get());
            }
        }
    }
}
