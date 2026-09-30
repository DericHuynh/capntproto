use super::*;
use capnp::{
    capability::Promise,
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
fn loaded_casts_preserve_capability_tables_and_release_only_replaced_references() {
    for list in [false, true] {
        let clones = Rc::new(Cell::new(0));
        let drops = Rc::new(Cell::new(0));
        let retained = {
            let loader = loader();
            let mut message = Message::new_default();
            let mut table = Vec::new();
            {
                let mut pointer = message.init_root::<any_pointer::Builder>();
                pointer.imbue_mut(&mut table);
                let mut value = if list {
                    pointer.init_as_list_of_any_struct(0, 1, 1).unwrap().get(0)
                } else {
                    pointer.init_as_any_struct(1, 1)
                };
                value
                    .reborrow()
                    .get_pointer_section()
                    .get(0)
                    .set_as_capability(Box::new(Opaque {
                        clones: clones.clone(),
                        drops: drops.clone(),
                    }));
            }
            {
                let mut pointer = message.get_root::<any_pointer::Builder>().unwrap();
                pointer.imbue_mut(&mut table);
                if list {
                    assert!(pointer
                        .reborrow()
                        .get_as::<any_list::Builder>()
                        .unwrap()
                        .get_as_loaded(element(&loader, "records"))
                        .is_err());
                    let value = pointer
                        .get_as::<any_list::Builder>()
                        .unwrap()
                        .get_as_loaded(element(&loader, "caps"))
                        .unwrap();
                    assert_eq!(value.len(), 1);
                } else {
                    assert!(pointer
                        .reborrow()
                        .get_as::<any_struct::Builder>()
                        .unwrap()
                        .get_as_loaded(root_schema(&loader))
                        .is_err());
                    let mut value = pointer
                        .get_as::<any_struct::Builder>()
                        .unwrap()
                        .get_as_loaded(structure(&loader, "cap"))
                        .unwrap();
                    value
                        .set_named("number", dynamic::Value::UInt32(42))
                        .unwrap();
                }
            }
            assert_eq!(table.len(), 1);
            assert_eq!(clones.get(), 0);
            assert_eq!(drops.get(), 0);
            let mut pointer = message.get_root_as_reader::<any_pointer::Reader>().unwrap();
            pointer.imbue(&table);
            let value = if list {
                let view = pointer
                    .get_as::<any_list::Reader>()
                    .unwrap()
                    .get_as_loaded(element(&loader, "caps"))
                    .unwrap();
                assert_eq!(clones.get(), 0);
                view.get(0).unwrap()
            } else {
                let view = pointer
                    .get_as::<any_struct::Reader>()
                    .unwrap()
                    .get_as_loaded(structure(&loader, "cap"))
                    .unwrap();
                assert_eq!(clones.get(), 0);
                view.get_named("payload").unwrap()
            };
            let dynamic::Value::Capability(client) = value else {
                panic!()
            };
            assert_eq!(clones.get(), 1, "only explicit extraction clones the hook");
            let retained = client
                .release_native::<capnp::capability::Client>()
                .map_err(|(error, _)| error)
                .unwrap();
            let mut pointer = message.get_root::<any_pointer::Builder>().unwrap();
            pointer.imbue_mut(&mut table);
            if list {
                pointer
                    .get_as::<any_list::Builder>()
                    .unwrap()
                    .get_as_loaded(element(&loader, "caps"))
                    .unwrap()
                    .set(0, dynamic::Value::Capability(dynamic::Client::null(None)))
                    .unwrap();
            } else {
                pointer
                    .get_as::<any_struct::Builder>()
                    .unwrap()
                    .get_as_loaded(structure(&loader, "cap"))
                    .unwrap()
                    .clear_named("payload")
                    .unwrap();
            }
            assert_eq!(clones.get(), 1);
            assert_eq!(
                drops.get(),
                1,
                "replacement releases the original table reference"
            );
            retained
        };
        assert_eq!(
            drops.get(),
            1,
            "extracted client outlives storage and metadata"
        );
        drop(retained);
        assert_eq!(drops.get(), 2);
    }
}

#[test]
fn erased_builders_preserve_projected_capabilities_and_unknown_fields() {
    for list in [false, true] {
        let clones = Rc::new(Cell::new(0));
        let drops = Rc::new(Cell::new(0));
        let retained = {
            let loader = loader();
            let mut message = Message::new_default();
            let mut table = Vec::new();
            {
                let mut pointer = message.init_root::<any_pointer::Builder>();
                pointer.imbue_mut(&mut table);
                let mut raw = if list {
                    pointer.init_as_list_of_any_struct(2, 2, 1).unwrap().get(0)
                } else {
                    pointer.init_as_any_struct(2, 2)
                };
                raw.get_data_section()[8] = 77;
                raw.get_pointer_section()
                    .get(1)
                    .set_as::<capnp::text::Owned>("unknown")
                    .unwrap();
                raw.get_pointer_section()
                    .get(0)
                    .set_as_capability(Box::new(Opaque {
                        clones: clones.clone(),
                        drops: drops.clone(),
                    }));
            }
            let mut pointer = message.get_root::<any_pointer::Builder>().unwrap();
            pointer.imbue_mut(&mut table);
            let mut raw = if list {
                let view = pointer
                    .get_as::<any_list::Builder>()
                    .unwrap()
                    .get_as_loaded(element(&loader, "caps"))
                    .unwrap();
                let erased = view.into_any_list();
                assert_eq!(erased.get_element_size(), E::InlineComposite);
                erased.get_as_struct_list().unwrap().get(0)
            } else {
                dynamic::Builder::new(pointer, structure(&loader, "cap"))
                    .unwrap()
                    .into_any_struct()
            };
            assert_eq!(clones.get(), 0);
            assert_eq!(drops.get(), 0);
            assert_eq!(raw.get_data_section()[8], 77);
            assert_eq!(
                raw.as_reader()
                    .get_pointer_section()
                    .get(1)
                    .get_as::<capnp::text::Reader>()
                    .unwrap(),
                "unknown"
            );
            let retained = raw
                .as_reader()
                .get_pointer_section()
                .get(0)
                .get_as_capability::<capnp::capability::Client>()
                .unwrap();
            assert_eq!(clones.get(), 1);
            raw.get_pointer_section().get(0).clear();
            assert_eq!(drops.get(), 1);
            assert_eq!(raw.get_data_section()[8], 77);
            assert_eq!(
                raw.as_reader()
                    .get_pointer_section()
                    .get(1)
                    .get_as::<capnp::text::Reader>()
                    .unwrap(),
                "unknown"
            );
            retained
        };
        assert_eq!(clones.get(), 1);
        assert_eq!(drops.get(), 1);
        drop(retained);
        assert_eq!(drops.get(), 2);
    }
}
