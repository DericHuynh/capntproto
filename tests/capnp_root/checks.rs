// Included in a standalone downstream crate; it intentionally has no direct
// dependency named `capnp` in the override cases.
pub use test_runtime::*;
pub mod wire {
    pub use crate::test_runtime::*;
}

macro_rules! check_legacy {
    ($module:path) => {{
        use bindings::{leaf_capnp as dep, roots_capnp as s};
        use $module as bindings;
        fn owned<T: test_runtime::traits::Owned>() {}
        owned::<s::service::Owned>();
        owned::<s::generic::Owned<dep::leaf::Owned>>();
        owned::<s::box_::Owned<test_runtime::text_list::Owned>>();

        assert_eq!(s::NAMES.get()?.get(1)?, "one");
        assert_eq!(s::BLOBS.get()?.get(0)?, &[0, 255]);
        assert_eq!(s::SAMPLE.get()?.get_leaf()?.get_label()?, "imported");
        let mut message = test_runtime::message::Builder::new_default();
        {
            let mut record = message.init_root::<s::record::Builder<'_>>();
            assert_eq!(record.reborrow_as_reader().get_names()?.get(0)?, "default");
            assert_eq!(record.reborrow_as_reader().get_blobs()?.get(0)?, &[0, 255]);
            record.reborrow().init_names(2).set(1, "second");
            record.reborrow().init_blobs(1).set(0, b"bytes");
            record
                .reborrow()
                .init_nested_names(1)
                .init(0, 1)
                .set(0, "nested");
            record
                .reborrow()
                .init_nested_blobs(1)
                .init(0, 1)
                .set(0, b"nested");
            record.reborrow().init_leaf().set_label("leaf");
            record
                .reborrow()
                .init_boxed()
                .initn_value(1)
                .set(0, "boxed");
            record.init_details().set_label("group");
        }
        let bytes = test_runtime::serialize::write_message_to_words(&message);
        let decoded = test_runtime::serialize::read_message(
            bytes.as_slice(),
            test_runtime::message::ReaderOptions::new(),
        )?;
        let record = decoded.get_root::<s::record::Reader<'_>>()?;
        assert_eq!(record.get_names()?.get(1)?, "second");
        assert_eq!(record.get_blobs()?.get(0)?, b"bytes");
        assert_eq!(record.get_nested_names()?.get(0)?.get(0)?, "nested");
        assert_eq!(record.get_nested_blobs()?.get(0)?.get(0)?, b"nested");
        assert_eq!(record.get_leaf()?.get_label()?, "leaf");
        assert_eq!(record.get_boxed()?.get_value()?.get(0)?, "boxed");
        assert_eq!(record.get_state()?, dep::State::Ready);
        match record.which()? {
            s::record::Details(details) => assert_eq!(details.get_label()?, "group"),
            _ => panic!("group was not selected"),
        }
        let dynamic: test_runtime::dynamic_value::Reader<'_> = record.into();
        assert!(format!("{dynamic:?}").contains("second"));
        println!("{}: legacy roundtrip passed", stringify!($module));
    }};
}

macro_rules! check_facade {
    ($module:path) => {{
        use bindings::roots_capnp::api::Record;
        use test_runtime::field_api::{Message, MessageView};
        use $module as bindings;
        let mut message = Message::<Record>::new()?;
        assert_eq!(message.read().names()?.get(0).transpose()?, Some("default"));
        message
            .edit()
            .names()
            .init_with(2, |i, name| name.copy_from(["one", "two"][i]))?;
        message
            .edit()
            .blobs()
            .init_with(1, |_, data| data.copy_from(b"facade"))?;
        let frozen = message.freeze();
        let bytes = frozen.to_vec();
        let view = MessageView::<Record>::from_unpacked(&bytes, Default::default())?;
        assert_eq!(view.read().names()?.get(1).transpose()?, Some("two"));
        assert_eq!(
            view.read().blobs()?.get(0).transpose()?,
            Some(b"facade".as_slice())
        );
        assert_eq!(
            view.read().project()?.names.get(1).transpose()?,
            Some("two")
        );
        let value = view.read().to_value(
            test_runtime::field_api::native::UnknownFields::Discard,
            Default::default(),
        )?;
        let restored = value.to_message(Default::default())?;
        assert_eq!(
            restored.read().blobs()?.get(0).transpose()?,
            Some(b"facade".as_slice())
        );
        println!(
            "{}: facade, projection and native value roundtrip passed",
            stringify!($module)
        );
    }};
}
