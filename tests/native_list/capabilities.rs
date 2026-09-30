use super::super::*;
use capnp::{
    any_pointer,
    capability::{FromClientHook, Promise},
    private::capability::{ClientHook, ParamsHook, ResultsHook},
    traits::{Imbue, ImbueMut},
};
use std::{cell::Cell, rc::Rc};

// Casting must not extract, clone, identify, resolve or call a capability.
// Only deliberate element extraction below may call add_ref().
struct Opaque {
    clones: Rc<Cell<usize>>,
    drops: Rc<Cell<usize>>,
}
impl Drop for Opaque {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
    }
}
impl ClientHook for Opaque {
    fn add_ref(&self) -> Box<dyn ClientHook> {
        self.clones.set(self.clones.get() + 1);
        Box::new(Self {
            clones: self.clones.clone(),
            drops: self.drops.clone(),
        })
    }
    fn new_call(
        &self,
        _: u64,
        _: u16,
        _: Option<capnp::MessageSize>,
    ) -> capnp::capability::Request<any_pointer::Owned, any_pointer::Owned> {
        panic!("unexpected call")
    }
    fn call(
        &self,
        _: u64,
        _: u16,
        _: Box<dyn ParamsHook>,
        _: Box<dyn ResultsHook>,
    ) -> Promise<(), capnp::Error> {
        panic!("unexpected call")
    }
    fn get_brand(&self) -> usize {
        panic!("unexpected brand")
    }
    fn get_ptr(&self) -> usize {
        panic!("unexpected identity")
    }
    fn get_resolved(&self) -> Option<Box<dyn ClientHook>> {
        panic!("unexpected resolution")
    }
    fn when_more_resolved(&self) -> Option<Promise<Box<dyn ClientHook>, capnp::Error>> {
        panic!("unexpected resolution")
    }
    fn when_resolved(&self) -> Promise<(), capnp::Error> {
        panic!("unexpected resolution")
    }
}

#[test]
fn loaded_and_compiled_capability_list_casts_only_read_metadata() {
    let clones = Rc::new(Cell::new(0));
    let drops = Rc::new(Cell::new(0));
    let retained = {
        let mut loader = unregistered();
        register(&mut loader, 2);
        let mut message = message::Builder::new_default();
        let mut table = Vec::new();
        {
            let mut pointer = message.init_root::<any_pointer::Builder>();
            pointer.imbue_mut(&mut table);
            let root = dynamic::Builder::init(pointer, loader.get(root_id()).unwrap()).unwrap();
            let mut loaded = root.init_list("caps", 1).unwrap();
            assert!(loaded.reborrow().downcast_native::<WrongCaps>().is_err());
            let mut native = loaded.reborrow().downcast_native::<Caps>().unwrap();
            native.set(
                0,
                Box::new(Opaque {
                    clones: clones.clone(),
                    drops: drops.clone(),
                }),
            );
            // Exercise both directions of compiled dynamic conversion too.
            let erased: capnp::dynamic_value::Builder = native.into();
            let native: capnp::capability_list::Builder<service::Client> = erased.downcast();
            assert_eq!(native.len(), 1);
            let erased: capnp::dynamic_value::Reader = native.into_reader().into();
            let native: capnp::capability_list::Reader<service::Client> = erased.downcast();
            assert_eq!(native.len(), 1);
            assert_eq!(clones.get(), 0);
            assert_eq!(drops.get(), 0);
            let mut loaded_again = loaded.reborrow().downcast_native::<Caps>().unwrap();
            assert_eq!(loaded_again.reborrow().len(), 1);
        }
        assert_eq!(
            table.len(),
            1,
            "native writes retain the original cap table"
        );
        let mut pointer = message.get_root_as_reader::<any_pointer::Reader>().unwrap();
        pointer.imbue(&table);
        let root = dynamic::Reader::new(pointer, loader.get(root_id()).unwrap()).unwrap();
        let dynamic::Value::List(list) = root.get_named("caps").unwrap() else {
            panic!()
        };
        assert!(list.clone().downcast_native::<WrongCaps>().is_err());
        let native = list.downcast_native::<Caps>().unwrap();
        assert_eq!(clones.get(), 0);
        assert_eq!(drops.get(), 0);
        let client = native.get(0).unwrap();
        assert_eq!(clones.get(), 1);
        client
    };
    assert_eq!(drops.get(), 1, "table releases its reference");
    drop(retained);
    assert_eq!(drops.get(), 2, "extracted client owns its reference");
}

struct Echo(Rc<Cell<bool>>);
impl Drop for Echo {
    fn drop(&mut self) {
        self.0.set(false);
    }
}
impl service::Server for Echo {
    async fn ping(
        self: Rc<Self>,
        params: service::PingParams,
        mut results: service::PingResults,
    ) -> capnp::Result<()> {
        results.get().set_value(params.get()?.get_value() + 1);
        Ok(())
    }
}

#[test]
fn extracted_native_capability_remains_callable_after_message_and_loader_drop() {
    use futures::task::LocalSpawnExt;
    let mut pool = futures::executor::LocalPool::new();
    let (executor, driver) = capnp_rpc::new_call_executor();
    pool.spawner()
        .spawn_local(async move {
            driver.await.unwrap();
        })
        .unwrap();
    let alive = Rc::new(Cell::new(true));
    let client = {
        let mut loader = unregistered();
        register(&mut loader, 2);
        let mut message = message::Builder::new_default();
        let mut table = Vec::new();
        let mut pointer = message.init_root::<any_pointer::Builder>();
        pointer.imbue_mut(&mut table);
        let root = dynamic::Builder::init(pointer, loader.get(root_id()).unwrap()).unwrap();
        let mut list = root
            .init_list("caps", 1)
            .unwrap()
            .downcast_native::<Caps>()
            .unwrap();
        let original: service::Client =
            capnp_rpc::new_client_with_executor(Echo(alive.clone()), executor);
        list.set(0, original.into_client_hook());
        list.get(0).unwrap()
    };
    assert!(alive.get());
    let mut request = client.ping_request();
    request.get().set_value(41);
    let response = pool.run_until(request.send().promise).unwrap();
    assert_eq!(response.get().unwrap().get_value(), 42);
    drop(response);
    drop(client);
    pool.run_until_stalled();
    assert!(!alive.get());
}

#[test]
fn interface_inheritance_does_not_make_list_native_types_interchangeable() {
    let mut loader = unregistered();
    loader
        .load_compiled_type_and_dependencies::<fixture::derived::Owned>()
        .unwrap();
    let mut message = message::Builder::new_default();
    let root = dynamic::Builder::init(message.init_root(), loader.get(root_id()).unwrap()).unwrap();
    let mut list = root.init_list("derived", 0).unwrap();
    assert!(list.reborrow().downcast_native::<Caps>().is_err());
    assert!(list.into_reader().downcast_native::<Caps>().is_err());
    let schema = loader
        .get(root_id())
        .unwrap()
        .field("derived")
        .unwrap()
        .get_type()
        .unwrap();
    assert!(schema
        .require_usable_as::<capnp::capability_list::Owned<fixture::derived::Client>>()
        .is_ok());
}
