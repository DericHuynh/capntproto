use capnp::field_api::{
    native::{Limits, UnknownFields},
    Entry, Message, MessageView,
};
use capnp::{Error, ErrorKind, Result};
use capntproto_test_support::field_api_capnp::{api::*, service};
use std::{cell::Cell, rc::Rc};

fn failure<T>(result: Result<T>) -> Error {
    result.err().expect("operation should fail")
}
fn contains_in_order(error: &Error, parts: &[&str]) {
    let mut text = error.extra.as_str();
    for part in parts {
        let at = text
            .find(part)
            .unwrap_or_else(|| panic!("missing {part:?} in {error}"));
        text = &text[at + part.len()..];
    }
}
fn corrupt(bytes: &mut [u8], text: &[u8]) {
    let at = bytes.windows(text.len()).position(|b| b == text).unwrap();
    bytes[at] = 255;
}

#[test]
fn malformed_fields_report_schema_names_even_when_rust_names_differ() -> Result<()> {
    let mut m = Message::<RenamedFields>::new()?;
    m.edit().label().copy_from("secret payload")?;
    let mut bytes = m.to_vec();
    corrupt(&mut bytes, b"secret payload");
    let view = MessageView::<RenamedFields>::from_unpacked(&bytes, Default::default())?;
    let raw_error: Error = view
        .read()
        .field(RenamedFields::LABEL)
        .wire_text()?
        .to_str()
        .unwrap_err()
        .into();
    for error in [
        failure(view.read().label()),
        failure(view.read().project()),
        failure(view.read().field(RenamedFields::LABEL).present()),
        failure(
            view.read()
                .to_value(UnknownFields::Discard, Limits::default()),
        ),
    ] {
        assert_eq!(error.kind, raw_error.kind);
        assert!(
            error
                .extra
                .contains("field-api.capnp:RenamedFields.read (@0)"),
            "{error}"
        );
        assert!(!error.extra.contains("secret payload"));
        assert!(!error.extra.contains(".label"));
    }
    Ok(())
}

#[test]
fn native_nested_list_errors_identify_the_element_and_child_field() -> Result<()> {
    let mut m = Message::<Person>::new()?;
    m.edit().phones().init_with(3, |i, mut phone| {
        phone.number().copy_from(["first", "broken", "last"][i])
    })?;
    let mut bytes = m.to_vec();
    corrupt(&mut bytes, b"broken");
    let view = MessageView::<Person>::from_unpacked(&bytes, Default::default())?;
    // Lazy projections do not inspect an unrelated list element.
    assert_eq!(view.read().project()?.phones.len(), 3);
    let direct = failure(view.read().phones()?.get(1).unwrap().number());
    let native = failure(
        view.read()
            .to_value(UnknownFields::Discard, Limits::default()),
    );
    assert_eq!(direct.kind, native.kind);
    contains_in_order(
        &native,
        &[
            "native decode field-api.capnp:Person",
            "Person.phones (@4)",
            "[1]",
            "PhoneNumber.number (@0)",
        ],
    );
    Ok(())
}

#[test]
fn native_encode_reports_nested_budget_and_union_context() -> Result<()> {
    let mut m = Message::<Person>::new()?;
    m.edit()
        .phones()
        .init_with(2, |_, mut p| p.number().copy_from("123"))?;
    let value = m
        .read()
        .to_value(UnknownFields::Discard, Limits::default())?;
    let error = failure(value.to_message(Limits {
        bytes: 3,
        ..Limits::default()
    }));
    contains_in_order(
        &error,
        &[
            "native encode",
            "Person.phones (@4)",
            "[1]",
            "PhoneNumber.number (@0)",
            "byte limit",
        ],
    );
    assert_eq!(
        value.to_message(Limits::default())?.read().phones()?.len(),
        2
    );

    let mut c = Message::<Choice>::new()?;
    c.edit()
        .details()
        .replace()
        .nested()
        .value()
        .copy_from("group text")?;
    let mut bytes = c.to_vec();
    corrupt(&mut bytes, b"group text");
    let view = MessageView::<Choice>::from_unpacked(&bytes, Default::default())?;
    let error = failure(
        view.read()
            .to_value(UnknownFields::Discard, Limits::default()),
    );
    contains_in_order(
        &error,
        &[
            "Choice.details:",
            "Choice.details.nested:",
            "Choice.details.nested.value (@6)",
        ],
    );
    Ok(())
}

#[test]
fn list_pointer_reads_and_cached_editors_retain_the_failing_index() -> Result<()> {
    let mut m = Message::<Person>::new()?;
    m.edit()
        .names()
        .init(2)?
        .get_mut(1)
        .unwrap()
        .copy_from("list text")?;
    m.edit()
        .names()
        .edit()?
        .get_mut(1)
        .unwrap()
        .edit()?
        .as_bytes_mut()[0] = 255;
    let error = failure(m.read().names()?.get(1).unwrap());
    assert!(error.extra.starts_with("[1]"));
    let error = failure(m.edit().names().edit()?.get_mut(1).unwrap().entry());
    assert!(error.extra.starts_with("[1]"));
    let error = failure(m.read().to_value(UnknownFields::Discard, Limits::default()));
    contains_in_order(&error, &["Person.names (@6)", "[1]"]);

    Ok(())
}

#[test]
fn handle_errors_name_the_field_without_selecting_or_publishing_it() -> Result<()> {
    let mut m = Message::<Choice>::new()?;
    let e = failure(m.read().text());
    assert_eq!(e.kind, ErrorKind::NotPresent);
    assert!(e.extra.contains("Choice.text (@2)"));
    let e = failure(m.edit().text().entry());
    assert_eq!(e.kind, ErrorKind::NotPresent);
    assert!(e.extra.contains("Choice.text (@2)"));
    let e = failure(m.edit().details().edit());
    assert_eq!(e.kind, ErrorKind::NotPresent);
    assert!(e.extra.contains("Choice.details"));
    assert_eq!(m.read().tag(), ChoiceUnionTag::Empty);
    m.edit().text().copy_from("original")?;
    let mut root = m.edit();
    let pending = root.text().stage_replace(3)?;
    let e = failure(pending.finish());
    assert_eq!(e.kind, ErrorKind::IncompleteFill);
    assert!(e.extra.contains("Choice.text (@2)"));
    assert_eq!(root.read().text()?, "original");
    let e = failure(root.text().replace_with(3, |b| {
        b.as_bytes_mut().fill(255);
        Ok(())
    }));
    assert!(matches!(e.kind, ErrorKind::TextContainsNonUtf8Data(_)));
    assert!(e.extra.contains("Choice.text (@2)"));
    assert_eq!(root.read().text()?, "original");
    Ok(())
}

#[test]
fn cached_upgrade_failure_and_scoped_fields_keep_context() -> Result<()> {
    use capnp::field_api::Schema;
    use capnp::private::{
        arena::{BuilderArena, BuilderArenaImpl},
        layout::{PointerBuilder, StructSize},
    };
    // Deliberately store an older child layout without materializing the new field.
    let mut arena = BuilderArenaImpl::new(capnp::message::HeapAllocator::new());
    let (segment, offset) = arena.allocate_anywhere(1);
    assert_eq!((segment, offset), (0, 0));
    let (data, _) = arena.get_segment_mut(0);
    let mut raw = PointerBuilder::get_root(&mut arena, 0, data).init_struct(Evolving::SIZE);
    raw.reborrow().get_pointer_field(0).init_struct(StructSize {
        data: 1,
        pointers: 0,
    });
    let mut facade = EvolvingMut::from(raw);
    let Entry::Occupied(entry) = facade.record().entry()? else {
        panic!("occupied")
    };
    let e = failure(entry.edit());
    assert_eq!(e.kind, ErrorKind::NeedsUpgrade);
    assert!(e.extra.contains("Evolving.record (@0)"));
    facade.record().ensure()?.extra().copy_from("upgraded")?;
    assert_eq!(facade.read().record()?.extra()?, "upgraded");
    let mut m = Message::<Person>::new()?;
    m.scoped_edit(|session| {
        let (mut root, _) = session.into_parts();
        let e = failure(root.field(Person::PAYLOAD).edit());
        assert_eq!(e.kind, ErrorKind::NotPresent);
        assert!(e.extra.contains("Person.payload (@5)"));
    });
    Ok(())
}

struct Server(Rc<Cell<usize>>);
impl service::Server for Server {}
impl Drop for Server {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}
#[test]
fn staged_callback_errors_preserve_metadata_and_release_candidate_capabilities() -> Result<()> {
    let drops = Rc::new(Cell::new(0));
    let cap: service::Client = capnp_rpc::new_client(Server(drops.clone()));
    let mut m = Message::<Transfer>::new()?;
    let error = failure(m.edit().destination().replace_with(|mut value| {
        value.cap().copy_from(cap)?;
        let mut error = Error::overloaded("application rejected the change".into());
        error.set_remote_trace("peer trace".into());
        error.set_detail(19, vec![1, 2, 3]);
        Err(error)
    }));
    assert_eq!(error.kind, ErrorKind::Overloaded);
    assert_eq!(error.remote_trace(), Some("peer trace"));
    assert_eq!(error.detail(19), Some([1, 2, 3].as_slice()));
    contains_in_order(
        &error,
        &[
            "Transfer.destination (@1)",
            "application rejected the change",
        ],
    );
    assert!(m.read().field(Transfer::DESTINATION).is_null());
    assert_eq!(drops.get(), 1);
    Ok(())
}
