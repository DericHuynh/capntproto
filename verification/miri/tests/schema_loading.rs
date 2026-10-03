use capnp::{
    introspect::Introspect,
    message::{self, AllocationStrategy, HeapAllocator},
    schema_capnp::node,
    schema_loader::{
        dynamic::{self, orphan::Editor, Value},
        Limits, SchemaLoader,
    },
    Result,
};
use capntproto_memory_checks::field_api_capnp::new_record;

fn allocator(far: bool) -> HeapAllocator {
    if far {
        HeapAllocator::new()
            .first_segment_words(1)
            .allocation_strategy(AllocationStrategy::FixedSize)
    } else {
        HeapAllocator::new()
    }
}

fn definition(extra: bool) -> message::Builder<HeapAllocator> {
    let mut message = message::Builder::new_default();
    let mut node = message.init_root::<node::Builder>();
    node.set_id(1);
    node.set_display_name("Record");
    let mut structure = node.init_struct();
    structure.set_pointer_count(1);
    structure.set_data_word_count(u16::from(extra));
    let mut fields = structure.init_fields(1 + u32::from(extra));
    {
        let mut child = fields.reborrow().get(0);
        child.set_name("child");
        child.reborrow().init_ordinal().set_explicit(0);
        let mut slot = child.init_slot();
        slot.reborrow().init_type().init_struct().set_type_id(2);
        slot.init_default_value().init_struct();
    }
    if extra {
        let mut value = fields.get(1);
        value.set_name("value");
        value.set_code_order(1);
        value.reborrow().init_ordinal().set_explicit(1);
        let mut slot = value.init_slot();
        slot.reborrow().init_type().set_uint64(());
        slot.init_default_value().set_uint64(42);
    }
    message
}

#[test]
fn loaded_schema_clones_and_failed_batches_preserve_owned_definitions() -> Result<()> {
    let mut loader = SchemaLoader::default();
    {
        let input = definition(false);
        let bytes = capnp::serialize::write_message_to_words(&input);
        let parsed = capnp::serialize::read_message(bytes.as_slice(), Default::default())?;
        loader.load(parsed.get_root::<node::Reader>()?)?;
    }
    // Source builders and parsed input are gone; both independent loader owners
    // must retain the copied schema, including its unresolved typed dependency.
    let old = loader.clone();
    assert!(old.get(2)?.is_stub());
    let mut incompatible = message::Builder::new_default();
    {
        let mut node = incompatible.init_root::<node::Builder>();
        node.set_id(2);
        node.init_enum();
    }
    let newer = definition(true);
    assert!(loader
        .load_batch([
            newer.get_root_as_reader()?,
            incompatible.get_root_as_reader()?
        ])
        .is_err());
    assert_eq!(loader.get(1)?.fields()?.len(), 1);
    assert!(loader.get(2)?.is_stub());
    loader.load(newer.get_root_as_reader()?)?;
    drop(newer);
    assert_eq!(old.get(1)?.fields()?.len(), 1);
    assert_eq!(loader.get(1)?.fields()?.len(), 2);
    {
        let older = definition(false);
        loader.load(older.get_root_as_reader()?)?;
    }
    assert_eq!(loader.get(1)?.fields()?.len(), 2);
    for far in [false, true] {
        let mut message = message::Builder::new(allocator(far));
        let mut root = dynamic::Builder::init(message.init_root(), loader.get(1)?)?;
        assert!(matches!(
            root.as_reader().get_named("value")?,
            Value::UInt64(42)
        ));
        root.set_named("value", Value::UInt64(u64::MAX))?;
        assert!(matches!(
            root.as_reader().get_named("value")?,
            Value::UInt64(u64::MAX)
        ));
    }
    drop(loader);
    assert_eq!(old.get(1)?.get_proto().get_display_name()?, "Record");
    let mut bounded = SchemaLoader::new(Limits {
        nodes: 1,
        ..Limits::default()
    });
    assert!(bounded
        .load(definition(false).get_root_as_reader()?)
        .is_err());
    assert!(bounded.is_empty());
    Ok(())
}

#[test]
fn loaded_orphans_require_native_registration_and_reject_foreign_field_authority() -> Result<()> {
    let compiled = new_record::Owned::introspect().as_struct_schema()?;
    let id = compiled.get_proto().get_id();
    for registered in [false, true] {
        let mut loader = SchemaLoader::default();
        loader.load(compiled.get_proto())?;
        if registered {
            loader.load_compiled_type_and_dependencies::<new_record::Owned>()?;
        }
        let foreign = loader.clone();
        for far in [false, true] {
            let mut message = message::Builder::new(allocator(far));
            let (mut root, token) =
                dynamic::Builder::init(message.init_root(), loader.get(id)?)?.with_orphanage();
            assert_eq!(
                root.as_reader()
                    .downcast_native::<new_record::Owned>()
                    .is_ok(),
                registered
            );
            let mut text = token.in_struct(&mut root)?.new_text(3)?;
            token.in_struct(&mut root)?.edit(&mut text, |value| {
                let Editor::Text(text) = value else {
                    panic!("expected text")
                };
                text.as_bytes_mut().copy_from_slice(b"abc");
                Ok(())
            })?;
            let text = root
                .adopt(foreign.get(id)?.field("extra")?, text)
                .unwrap_err()
                .orphan;
            let text = root.adopt_named("value", text).unwrap_err().orphan;
            root.adopt_named("extra", text).unwrap();
            let Value::Text(text) = root.as_reader().get_named("extra")? else {
                panic!("expected text")
            };
            let address = text.as_bytes().as_ptr();
            let text = root.disown_named("extra", &token)?;
            let mut text = text.release_as::<capnp::text::Owned>().unwrap();
            token.in_struct(&mut root)?.read_typed(&mut text, |text| {
                assert_eq!(text.as_bytes().as_ptr(), address);
                Ok(())
            })?;
            token.in_struct(&mut root)?.resize_typed(&mut text, 5)?;
            token.in_struct(&mut root)?.read_typed(&mut text, |text| {
                assert_eq!(text.as_bytes(), b"abc\0\0");
                Ok(())
            })?;
            root.adopt_named("extra", text.into_dynamic()).unwrap();
            let mut orphan = token.in_struct(&mut root)?.new_struct(loader.get(id)?)?;
            token.in_struct(&mut root)?.edit(&mut orphan, |value| {
                let Editor::Struct(mut record) = value else {
                    panic!("expected struct")
                };
                record.set_named("value", Value::UInt64(73))?;
                record.set_named("extra", Value::Text("owned child".into()))
            })?;
            match orphan.release_as::<new_record::Owned>() {
                Ok(mut typed) => {
                    assert!(registered);
                    token
                        .in_struct(&mut root)?
                        .read_typed(&mut typed, |record| {
                            assert_eq!(record.get_value(), 73);
                            assert_eq!(record.get_extra()?, "owned child");
                            Ok(())
                        })?;
                }
                Err(error) => {
                    assert!(!registered);
                    let mut orphan = error.orphan;
                    token.in_struct(&mut root)?.read(&mut orphan, |value| {
                        let Value::Struct(record) = value else {
                            panic!("expected struct")
                        };
                        assert!(matches!(record.get_named("value")?, Value::UInt64(73)));
                        Ok(())
                    })?;
                }
            }
        }
    }
    Ok(())
}
