use capnp::{
    capability::Promise,
    dynamic_capability as dynamic, dynamic_value as value,
    introspect::{Introspect, Type, TypeVariant},
    traits::{Imbue, ImbueMut},
    Error,
};
use reproto_test_support::dynamic_test_capnp::{
    base, brand_envelope, derived, marker, parcel, scope, wrap,
};
use std::{cell::Cell, rc::Rc};
type Text = capnp::text::Owned;
type Data = capnp::data::Owned;

#[test]
fn brands_include_unused_nested_and_enclosing_parameters() {
    assert!(!marker::Client::<Text>::schema()
        .equals(marker::Client::<Data>::schema())
        .unwrap());
    assert!(!base::Client::<Text>::schema()
        .extends(base::Client::<Data>::schema())
        .unwrap());
    assert!(derived::Client::<Text>::schema()
        .extends(base::Client::<Text>::schema())
        .unwrap());
    assert!(!derived::Client::<Text>::schema()
        .extends(base::Client::<Data>::schema())
        .unwrap());
    assert!(wrap::Client::<Text>::schema()
        .extends(base::Client::<parcel::Owned<Text>>::schema())
        .unwrap());
    assert!(!wrap::Client::<Text>::schema()
        .extends(base::Client::<parcel::Owned<Data>>::schema())
        .unwrap());
    type A = scope::inner::Owned<Text, Data>;
    type B = scope::inner::Owned<Data, Text>;
    assert!(A::introspect().equals(A::introspect()).unwrap());
    assert!(!A::introspect().equals(B::introspect()).unwrap());
    type C = capnp::struct_list::Owned<parcel::Owned<Text>>;
    type D = capnp::struct_list::Owned<parcel::Owned<Data>>;
    assert!(!marker::Client::<C>::schema()
        .equals(marker::Client::<D>::schema())
        .unwrap());
    assert!(bool::introspect().equals(bool::introspect()).unwrap());
    assert!(!Type::list_of(bool::introspect())
        .equals(bool::introspect())
        .unwrap());
    // Older struct bindings have no complete generic brand. Strict operations
    // refuse to guess, while the explicitly loose/native path remains available.
    let TypeVariant::Struct(raw) = A::introspect().which() else {
        panic!()
    };
    assert!(!Type::from(TypeVariant::Struct(raw))
        .equals(A::introspect())
        .unwrap());
}

#[test]
fn recursive_custom_brand_metadata_is_bounded() {
    fn recursive(_: u16) -> Type {
        let TypeVariant::Interface(mut raw) = marker::Owned::<Text>::introspect().which() else {
            panic!()
        };
        raw.brand = capnp::introspect::Brand::new(1, recursive);
        TypeVariant::Interface(raw).into()
    }
    assert!(recursive(0).equals(recursive(0)).is_err());
    fn cyclic_parent(_: u16) -> Type {
        let TypeVariant::Interface(mut raw) = wrap::Owned::<Text>::introspect().which() else {
            panic!()
        };
        raw.superclass = cyclic_parent;
        TypeVariant::Interface(raw).into()
    }
    let TypeVariant::Interface(raw) = cyclic_parent(0).which() else {
        panic!()
    };
    let cycle = capnp::schema::InterfaceSchema::new(raw);
    assert!(cycle
        .find_superclass(base::Client::<Data>::schema().get_proto().get_id())
        .is_err());
    assert!(cycle.find_method_by_name("missing").is_err());
    assert!(cycle.extends(base::Client::<Data>::schema()).is_err());
}

#[test]
fn reflected_structs_lists_and_fields_preserve_brands() {
    let mut source = capnp::message::Builder::new_default();
    let mut root = source.init_root::<parcel::Builder<Text>>();
    root.set_value("hello").unwrap();
    let good = value::Reader::from(root.into_reader());
    let mut source2 = capnp::message::Builder::new_default();
    let mut root2 = source2.init_root::<parcel::Builder<Data>>();
    root2.set_value(&b"bad"[..]).unwrap();
    let bad = value::Reader::from(root2.into_reader());
    let mut target = capnp::message::Builder::new_default();
    let root = target.init_root::<brand_envelope::Builder>();
    let mut target = value::Builder::from(root).downcast::<capnp::dynamic_struct::Builder>();
    target.set_named("item", good.clone()).unwrap();
    assert!(target.set_named("item", bad.clone()).is_err());
    let item = target
        .reborrow_as_reader()
        .get_named("item")
        .unwrap()
        .downcast::<capnp::dynamic_struct::Reader>();
    assert!(item
        .get_schema()
        .equals(
            parcel::Owned::<Text>::introspect()
                .as_struct_schema()
                .unwrap()
        )
        .unwrap());
    assert_eq!(
        item.get_named("value")
            .unwrap()
            .downcast::<capnp::text::Reader>()
            .to_str()
            .unwrap(),
        "hello"
    );
    let wrong_field = parcel::Owned::<Data>::introspect()
        .as_struct_schema()
        .unwrap()
        .get_field_by_name("value")
        .unwrap();
    assert!(item.get(wrong_field).is_err());
    let mut list = target
        .reborrow()
        .initn_named("items", 1)
        .unwrap()
        .downcast::<capnp::dynamic_list::Builder>();
    list.set(0, good).unwrap();
    assert!(list.set(0, bad).is_err());
    let item = list
        .into_reader()
        .get(0)
        .unwrap()
        .downcast::<capnp::dynamic_struct::Reader>();
    assert!(item
        .get_schema()
        .equals(
            parcel::Owned::<Text>::introspect()
                .as_struct_schema()
                .unwrap()
        )
        .unwrap());
    assert_eq!(
        item.get_named("value")
            .unwrap()
            .downcast::<capnp::text::Reader>()
            .to_str()
            .unwrap(),
        "hello"
    );
}

struct Target {
    schema: capnp::schema::InterfaceSchema,
    value: u32,
    alive: Rc<Cell<bool>>,
}
impl Drop for Target {
    fn drop(&mut self) {
        self.alive.set(false);
    }
}
impl dynamic::Server for Target {
    fn get_schema(&self) -> capnp::schema::InterfaceSchema {
        self.schema
    }
    fn allow_cancellation(&self) -> bool {
        true
    }
    fn call(
        self: Rc<Self>,
        _: capnp::schema::Method,
        mut context: dynamic::CallContext,
    ) -> Promise<(), Error> {
        Promise::from_future(
            async move { context.get_results()?.set_named("value", self.value.into()) },
        )
    }
}

#[tokio::test(flavor = "current_thread")]
async fn checked_and_native_casts_have_explicit_different_contracts() {
    let cap = capnp_rpc::new_dynamic_client(Target {
        schema: derived::Client::<Text>::schema(),
        value: 7,
        alive: Rc::new(Cell::new(true)),
    });
    assert!(cap.cast::<derived::Client<Data>>().is_err());
    assert!(cap.cast_native::<derived::Client<Data>>().is_ok());
    assert!(cap.cast_native::<base::Client<Text>>().is_err());
    assert!(cap
        .upcast(base::Client::<Text>::schema())
        .unwrap()
        .cast_native::<base::Client<Text>>()
        .is_ok());
    let method = base::Client::<Data>::schema()
        .get_method_by_name("transform")
        .unwrap();
    assert!(cap.new_request_for(method, None).is_err());
    let remote = cap.new_request("transform", None).unwrap().send();
    let wrong_result_field = base::Client::<Data>::schema()
        .get_method_by_name("transform")
        .unwrap()
        .get_result_type()
        .unwrap()
        .get_field_by_name("cap")
        .unwrap();
    assert!(remote.pipeline.get(wrong_result_field).is_err());
    assert_eq!(
        remote
            .promise
            .await
            .unwrap()
            .get()
            .unwrap()
            .get_named("value")
            .unwrap()
            .downcast::<u32>(),
        7
    );
}

type Message = (
    capnp::message::Builder<capnp::message::HeapAllocator>,
    capnp::private::layout::CapTable,
);
fn message() -> Message {
    (capnp::message::Builder::new_default(), Default::default())
}
fn write_slot(
    msg: &mut Message,
    location: usize,
    cap: dynamic::Client,
    compatible: bool,
) -> capnp::Result<()> {
    use reproto_test_support::dynamic_test_capnp::compound_envelope;
    if location < 2 {
        let mut root = msg.0.get_root::<brand_envelope::Builder>()?;
        root.imbue_mut(&mut msg.1);
        let mut root = value::Builder::from(root).downcast::<capnp::dynamic_struct::Builder>();
        if location == 0 {
            root.set_named("cap", cap.into())
        } else {
            root.get_named("caps")?
                .downcast::<capnp::dynamic_list::Builder>()
                .set(0, cap.into())
        }
    } else {
        // Copying a branded struct/list element must retain the capability,
        // reject incompatible arguments, and release the temporary source table.
        let mut src = message();
        let value = if compatible {
            let mut root = src.0.init_root::<parcel::Builder<base::Owned<Text>>>();
            root.imbue_mut(&mut src.1);
            root.set_value(cap.cast::<base::Client<Text>>()?)?;
            value::Reader::from(root.into_reader())
        } else {
            let mut root = src.0.init_root::<parcel::Builder<base::Owned<Data>>>();
            root.imbue_mut(&mut src.1);
            root.set_value(cap.cast::<base::Client<Data>>()?)?;
            value::Reader::from(root.into_reader())
        };
        let mut root = msg.0.get_root::<compound_envelope::Builder>()?;
        root.imbue_mut(&mut msg.1);
        let mut root = value::Builder::from(root).downcast::<capnp::dynamic_struct::Builder>();
        if location == 2 {
            root.set_named("item", value)
        } else {
            root.get_named("items")?
                .downcast::<capnp::dynamic_list::Builder>()
                .set(0, value)
        }
    }
}
fn read_slot(msg: &Message, location: usize) -> Option<dynamic::Client> {
    use reproto_test_support::dynamic_test_capnp::compound_envelope;
    let value = if location < 2 {
        let mut root = msg
            .0
            .get_root_as_reader::<brand_envelope::Reader>()
            .unwrap();
        root.imbue(&msg.1);
        let root = value::Reader::from(root).downcast::<capnp::dynamic_struct::Reader>();
        if location == 0 {
            if root
                .which()
                .unwrap()
                .unwrap()
                .get_proto()
                .get_name()
                .unwrap()
                == "sentinel"
            {
                return None;
            }
            root.get_named("cap").unwrap()
        } else {
            root.get_named("caps")
                .unwrap()
                .downcast::<capnp::dynamic_list::Reader>()
                .get(0)
                .unwrap()
        }
    } else {
        let mut root = msg
            .0
            .get_root_as_reader::<compound_envelope::Reader>()
            .unwrap();
        root.imbue(&msg.1);
        let root = value::Reader::from(root).downcast::<capnp::dynamic_struct::Reader>();
        let parcel = if location == 2 {
            root.get_named("item").unwrap()
        } else {
            root.get_named("items")
                .unwrap()
                .downcast::<capnp::dynamic_list::Reader>()
                .get(0)
                .unwrap()
        };
        parcel
            .downcast::<capnp::dynamic_struct::Reader>()
            .get_named("value")
            .unwrap()
    };
    let cap = value.downcast::<dynamic::Client>();
    cap.as_client().ok().map(|_| cap.clone())
}
fn clear_slot(msg: &mut Message, location: usize) {
    if location < 2 {
        let mut root = msg.0.get_root::<brand_envelope::Builder>().unwrap();
        root.imbue_mut(&mut msg.1);
        let mut root = value::Builder::from(root).downcast::<capnp::dynamic_struct::Builder>();
        if location == 0 {
            root.clear_named("cap").unwrap();
        } else {
            root.get_named("caps")
                .unwrap()
                .downcast::<capnp::dynamic_list::Builder>()
                .set(
                    0,
                    dynamic::Client::null(Some(base::Client::<Text>::schema())).into(),
                )
                .unwrap();
        }
    } else {
        let mut root = msg
            .0
            .get_root::<reproto_test_support::dynamic_test_capnp::compound_envelope::Builder>()
            .unwrap();
        root.imbue_mut(&mut msg.1);
        let mut root = value::Builder::from(root).downcast::<capnp::dynamic_struct::Builder>();
        if location == 2 {
            root.clear_named("item").unwrap();
        } else {
            root.get_named("items")
                .unwrap()
                .downcast::<capnp::dynamic_list::Builder>()
                .get(0)
                .unwrap()
                .downcast::<capnp::dynamic_struct::Builder>()
                .clear_named("value")
                .unwrap();
        }
    }
}
async fn trace(
    steps: &[serde_json::Value],
    compatible: bool,
    empty: bool,
    location: usize,
    far: bool,
) {
    let alive = [Rc::new(Cell::new(true)), Rc::new(Cell::new(true))];
    let first = capnp_rpc::new_dynamic_client(Target {
        schema: derived::Client::<Text>::schema(),
        value: 1,
        alive: alive[0].clone(),
    });
    let second = capnp_rpc::new_dynamic_client(Target {
        schema: if compatible {
            derived::Client::<Text>::schema()
        } else {
            derived::Client::<Data>::schema()
        },
        value: 2,
        alive: alive[1].clone(),
    });
    let ptrs = [
        first.as_client().unwrap().hook.get_ptr(),
        second.as_client().unwrap().hook.get_ptr(),
    ];
    let mut roots = Some([first, second]);
    let mut msg = (
        capnp::message::Builder::new(
            capnp::message::HeapAllocator::new().first_segment_words(if far { 1 } else { 1024 }),
        ),
        Default::default(),
    );
    if location < 2 {
        let mut root = msg.0.init_root::<brand_envelope::Builder>();
        root.set_sentinel(42);
        root.init_caps(1);
    } else {
        msg.0
            .init_root::<reproto_test_support::dynamic_test_capnp::compound_envelope::Builder>()
            .init_items(1);
    }
    if !empty {
        write_slot(&mut msg, location, roots.as_ref().unwrap()[0].clone(), true).unwrap();
    }
    let mut msg = Some(msg);
    let mut held = None;
    let mut accepted = false;
    let mut union_was_set = !empty;
    let mut result = 0;
    for step in steps {
        match step["action"].as_str().unwrap() {
            "assign" => {
                accepted = write_slot(
                    msg.as_mut().unwrap(),
                    location,
                    roots.as_ref().unwrap()[1].clone(),
                    compatible,
                )
                .is_ok();
            }
            "clear" => {
                clear_slot(msg.as_mut().unwrap(), location);
            }
            "read" => {
                held = read_slot(msg.as_ref().unwrap(), location);
            }
            "drop-root" => {
                roots.take();
            }
            "drop-message" => {
                msg.take();
            }
            "drop-held" => {
                held.take();
            }
            "use" => {
                result = held
                    .as_ref()
                    .unwrap()
                    .new_request("transform", None)
                    .unwrap()
                    .send()
                    .promise
                    .await
                    .unwrap()
                    .get()
                    .unwrap()
                    .get_named("value")
                    .unwrap()
                    .downcast::<u32>();
            }
            _ => panic!(),
        }
        union_was_set |= accepted;
        let expected = step["state"].as_array().unwrap();
        assert_eq!(accepted, expected[1] == 1, "acceptance: {step}");
        assert_eq!(
            alive[0].get(),
            expected[9] == 1,
            "first owner: {step}, location={location}"
        );
        assert_eq!(
            alive[1].get(),
            expected[10] == 1,
            "second owner: {step}, location={location}"
        );
        assert_eq!(result, expected[8].as_u64().unwrap() as u32);
        if let Some(ref msg) = msg {
            let cap = read_slot(msg, location);
            let slot = expected[2].as_u64().unwrap() as usize;
            assert_eq!(cap.is_some(), slot != 0);
            if let Some(cap) = cap {
                assert_eq!(cap.as_client().unwrap().hook.get_ptr(), ptrs[slot - 1]);
            }
            if location == 0 && !union_was_set {
                assert!(matches!(
                    msg.0
                        .get_root_as_reader::<brand_envelope::Reader>()
                        .unwrap()
                        .which()
                        .unwrap(),
                    brand_envelope::Which::Sentinel(42)
                ));
            }
        }
        if let Some(ref held) = held {
            assert_eq!(
                held.as_client().unwrap().hook.get_ptr(),
                ptrs[expected[5].as_u64().unwrap() as usize - 1]
            );
        }
    }
}
#[tokio::test(flavor = "current_thread")]
async fn failed_brand_assignment_preserves_union_and_capability_ownership() {
    let steps = serde_json::json!([
        {"action":"assign","state":[1,0,1,1,1,0,0,0,0,1,1]},
        {"action":"read","state":[1,0,1,1,1,1,1,0,0,1,1]},
        {"action":"drop-root","state":[1,0,1,0,1,1,1,0,0,1,0]},
        {"action":"drop-message","state":[1,0,1,0,0,1,1,0,0,1,0]},
        {"action":"use","state":[1,0,1,0,0,1,1,1,1,1,0]}
    ]);
    for location in 0..4 {
        for far in [false, true] {
            trace(steps.as_array().unwrap(), false, false, location, far).await;
        }
    }
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_dynamic_brand_traces() {
    let path = reproto_test_support::verification::input("REPROTO_DYNAMIC_BRAND_TRACES")
        .expect("prepare verified trace corpus");
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    for case in cases {
        for location in 0..4 {
            for far in [false, true] {
                trace(
                    case["steps"].as_array().unwrap(),
                    case["compatible"].as_bool().unwrap(),
                    case["empty"].as_bool().unwrap(),
                    location,
                    far,
                )
                .await;
            }
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn replacing_capability_trees_releases_table_entries_and_preserves_owned_reads() {
    type CapList = capnp::capability_list::Owned<base::Client<Text>>;
    type CapParcel = parcel::Owned<base::Owned<Text>>;
    for far in [false, true] {
        for shape in 0..5 {
            for replacement in 0..7 {
                let alive = Rc::new(Cell::new(true));
                let cap = capnp_rpc::new_dynamic_client(Target {
                    schema: base::Client::<Text>::schema(),
                    value: 7,
                    alive: alive.clone(),
                });
                let native = cap.cast::<base::Client<Text>>().unwrap();
                let mut msg = capnp::message::Builder::new(
                    capnp::message::HeapAllocator::new().first_segment_words(if far {
                        1
                    } else {
                        1024
                    }),
                );
                let mut table = capnp::private::layout::CapTable::new();
                {
                    let mut root = msg.init_root::<capnp::any_pointer::Builder>();
                    root.imbue_mut(&mut table);
                    match shape {
                        0 => root.set_as::<base::Owned<Text>>(native).unwrap(),
                        1 => root
                            .init_as::<parcel::Builder<base::Owned<Text>>>()
                            .set_value(native)
                            .unwrap(),
                        2 => root
                            .initn_as::<capnp::capability_list::Builder<base::Client<Text>>>(1)
                            .set(0, native.client.hook),
                        3 => root
                            .initn_as::<capnp::struct_list::Builder<CapParcel>>(1)
                            .get(0)
                            .set_value(native)
                            .unwrap(),
                        4 => root
                            .initn_as::<capnp::list_list::Builder<CapList>>(1)
                            .init(0, 1)
                            .set(0, native.client.hook),
                        _ => unreachable!(),
                    }
                }
                assert_eq!(table.iter().filter(|cap| cap.is_some()).count(), 1);
                let replacement_alive = Rc::new(Cell::new(true));
                {
                    let mut root = msg.get_root::<capnp::any_pointer::Builder>().unwrap();
                    root.imbue_mut(&mut table);
                    match replacement {
                        0 => root.clear(),
                        1 => root.set_as::<capnp::text::Owned>("new text").unwrap(),
                        2 => root.set_as::<capnp::data::Owned>(&b"new data"[..]).unwrap(),
                        3 => {
                            root.init_as::<parcel::Builder<Text>>();
                        }
                        4 => {
                            root.initn_as::<capnp::primitive_list::Builder<u64>>(8);
                        }
                        5 => {
                            let next = capnp_rpc::new_dynamic_client(Target {
                                schema: base::Client::<Text>::schema(),
                                value: 8,
                                alive: replacement_alive.clone(),
                            });
                            root.set_as::<base::Owned<Text>>(
                                next.cast::<base::Client<Text>>().unwrap(),
                            )
                            .unwrap();
                        }
                        6 => {
                            let mut empty = capnp::message::Builder::new_default();
                            let null = empty
                                .init_root::<capnp::any_pointer::Builder>()
                                .into_reader();
                            root.set_as::<capnp::any_pointer::Owned>(null).unwrap();
                        }
                        _ => unreachable!(),
                    }
                }
                assert_eq!(
                    table.iter().filter(|cap| cap.is_some()).count(),
                    usize::from(replacement == 5)
                );
                assert!(alive.get());
                assert_eq!(
                    cap.new_request("transform", None)
                        .unwrap()
                        .send()
                        .promise
                        .await
                        .unwrap()
                        .get()
                        .unwrap()
                        .get_named("value")
                        .unwrap()
                        .downcast::<u32>(),
                    7
                );
                drop(cap);
                assert!(
                    !alive.get(),
                    "shape={shape}, replacement={replacement}, far={far}"
                );
                if replacement == 5 {
                    assert!(replacement_alive.get());
                    let mut root = msg.get_root::<capnp::any_pointer::Builder>().unwrap();
                    root.imbue_mut(&mut table);
                    root.clear();
                    assert!(!replacement_alive.get());
                }
            }
        }
    }
}
