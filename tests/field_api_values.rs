use capnp::field_api::{
    native::{Limits, UnknownFields},
    Message, MessageView,
};
use reproto_test_support::field_api_capnp::{api::*, service};
use std::{cell::Cell, rc::Rc};

fn limits() -> Limits {
    Limits::default()
}
struct Server(Rc<Cell<usize>>);
impl service::Server for Server {}
impl Drop for Server {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}
fn client(drops: &Rc<Cell<usize>>) -> service::Client {
    capnp_rpc::new_client(Server(drops.clone()))
}

#[test]
fn projection_borrows_payloads_and_decodes_only_selected_union() -> capnp::Result<()> {
    let mut m = Message::<Person>::new()?;
    m.edit().name().copy_from("borrowed")?;
    m.edit().employment().school().copy_from("university")?;
    let view = m.read().project()?;
    assert_eq!(view.name.as_ptr(), m.read().name()?.as_ptr());
    assert_eq!(view.id, 42);
    assert!(matches!(
        view.employment,
        person::EmploymentUnionRef::School("university")
    ));
    let mut choice = Message::<Choice>::new()?;
    choice
        .edit()
        .details()
        .replace()
        .label()
        .copy_from("nested")?;
    match choice.read().project()?.__union {
        ChoiceUnionRef::Details(group) => assert_eq!(group.project()?.label, "nested"),
        _ => panic!("wrong selected arm"),
    }
    let mut bytes = m.to_vec();
    let at = bytes.windows(8).position(|b| b == b"borrowed").unwrap();
    bytes[at] = 255;
    let malformed = MessageView::<Person>::from_unpacked(&bytes, Default::default())?;
    assert!(malformed.read().project().is_err());
    assert!(malformed
        .read()
        .to_value(UnknownFields::Discard, limits())
        .is_err());
    Ok(())
}

#[test]
fn native_roundtrip_preserves_absence_defaults_lists_groups_and_open_enums() -> capnp::Result<()> {
    let mut m = Message::<Person>::new()?;
    m.edit()
        .previous_address()
        .init()?
        .city()
        .copy_from("actual")?;
    m.edit()
        .names()
        .init(3)?
        .get_mut(1)
        .unwrap()
        .copy_from("")?;
    m.edit().phones().init_with(1, |_, mut p| {
        p.kind().set(PhoneKind::Unknown(60000));
        p.number().copy_from("123")
    })?;
    m.edit().matrix().init_with(2, |i, row| {
        if i == 0 {
            row.init_with(2, |i, v| {
                v.set(i as u32);
                Ok(())
            })
        } else {
            Ok(())
        }
    })?;
    m.edit().employment().employer().copy_from("native")?;
    let mut value = m.read().to_value(UnknownFields::Discard, limits())?;
    assert!(value.name.is_none());
    assert!(value.address.is_none());
    assert_eq!(
        value.previous_address.as_ref().unwrap().city.as_deref(),
        Some("actual")
    );
    assert_eq!(
        value.names.as_ref().unwrap(),
        &[None, Some(String::new()), None]
    );
    assert_eq!(value.matrix.as_ref().unwrap(), &[Some(vec![0, 1]), None]);
    assert_eq!(
        value.phones.as_ref().unwrap()[0].kind,
        PhoneKind::Unknown(60000)
    );
    value.id = 900;
    drop(m);
    let restored = value.to_message(limits())?;
    assert!(restored.read().field(Person::NAME).is_null());
    assert_eq!(restored.read().name()?, "anonymous");
    assert_eq!(restored.read().address()?.city()?, "default city");
    assert_eq!(restored.read().id(), 900);
    assert_eq!(
        restored.read().phones()?.get(0).unwrap().kind(),
        PhoneKind::Unknown(60000)
    );
    assert!(matches!(
        restored.read().employment()?,
        person::EmploymentUnionRef::Employer("native")
    ));
    let mut choice = Message::<Choice>::new()?;
    choice.edit().untouched().set(37);
    let mut root = choice.edit();
    let mut group = root.details().replace();
    group.count().set(8);
    group.nested().value().copy_from("nested")?;
    let cv = choice.read().to_value(UnknownFields::Discard, limits())?;
    let cr = cv.to_message(limits())?;
    assert_eq!(cr.read().untouched(), 37);
    assert_eq!(cr.read().details()?.count(), 8);
    assert!(matches!(
        cr.read().details()?.nested()?,
        choice::details::NestedUnionRef::Value("nested")
    ));
    Ok(())
}

#[test]
fn native_capability_and_generic_payload_ownership_is_independent() -> capnp::Result<()> {
    let drops = Rc::new(Cell::new(0));
    let cap = client(&drops);
    let identity = cap.client.hook.get_ptr();
    let mut m = Message::<Types>::new()?;
    m.edit().cap().copy_from(cap)?;
    m.edit()
        .r#box()
        .ensure()?
        .value()
        .copy_from("generic".into())?;
    let value = m.read().to_value(UnknownFields::Discard, limits())?;
    assert_eq!(
        value
            .r#box
            .as_ref()
            .unwrap()
            .value
            .as_ref()
            .unwrap()
            .read()?
            .to_str()?,
        "generic"
    );
    m.edit()
        .r#box()
        .edit()?
        .value()
        .copy_from("changed".into())?;
    drop(m);
    assert_eq!(drops.get(), 0);
    assert_eq!(value.cap.as_ref().unwrap().client.hook.get_ptr(), identity);
    let restored = value.to_message(limits())?;
    drop(value);
    assert_eq!(drops.get(), 0);
    assert_eq!(restored.read().cap()?.client.hook.get_ptr(), identity);
    assert_eq!(restored.read().r#box()?.value()?.to_str()?, "generic");
    drop(restored);
    assert_eq!(drops.get(), 1);
    Ok(())
}

#[test]
fn conversion_limits_bound_recursive_and_zero_size_expansion() -> capnp::Result<()> {
    let mut m = Message::<Recursive>::new()?;
    m.edit().next().init()?.next().init()?.value().set(42);
    m.edit().voids().init(5000)?;
    assert!(m
        .read()
        .to_value(
            UnknownFields::Discard,
            Limits {
                items: 100,
                ..limits()
            }
        )
        .is_err());
    assert!(m
        .read()
        .to_value(
            UnknownFields::Discard,
            Limits {
                depth: 2,
                ..limits()
            }
        )
        .is_err());
    let value = m.read().to_value(UnknownFields::Discard, limits())?;
    assert_eq!(
        value.next.as_ref().unwrap().next.as_ref().unwrap().value,
        42
    );
    assert!(value
        .next
        .as_ref()
        .unwrap()
        .next
        .as_ref()
        .unwrap()
        .next
        .is_none());
    assert!(value
        .to_message(Limits {
            depth: 2,
            ..limits()
        })
        .is_err());
    assert!(value
        .to_message(Limits {
            items: 100,
            ..limits()
        })
        .is_err());
    assert_eq!(value.to_message(limits())?.read().voids()?.len(), 5000);
    let mut text = Message::<Address>::new()?;
    text.edit().city().copy_from("long")?;
    assert!(text
        .read()
        .to_value(
            UnknownFields::Discard,
            Limits {
                bytes: 3,
                ..limits()
            }
        )
        .is_err());
    Ok(())
}

#[test]
fn unknown_fields_require_loss_opt_in_and_unknown_union_is_rejected() -> capnp::Result<()> {
    let mut newer = Message::<NewRecord>::new()?;
    newer.edit().value().set(77);
    newer.edit().extra().copy_from("future field")?;
    let bytes = newer.to_vec();
    let older = MessageView::<OldRecord>::from_unpacked(&bytes, Default::default())?;
    let value = older.read().to_value(UnknownFields::Discard, limits())?;
    let output = value.to_message(limits())?.to_vec();
    let as_new = MessageView::<NewRecord>::from_unpacked(&output, Default::default())?;
    assert_eq!(as_new.read().value(), 77);
    assert!(as_new.read().field(NewRecord::EXTRA).is_null());
    let mut c = Message::<Choice>::new()?;
    let empty = c.to_vec();
    c.edit().text().copy_from("")?;
    let mut bytes = c.to_vec();
    let offset = (16..32).find(|&i| empty[i] != bytes[i]).unwrap();
    bytes[offset..offset + 2].copy_from_slice(&60000u16.to_le_bytes());
    let unknown = MessageView::<Choice>::from_unpacked(&bytes, Default::default())?;
    assert!(matches!(
        unknown.read().project()?.__union,
        ChoiceUnionRef::Unknown(60000)
    ));
    let error = unknown
        .read()
        .to_value(UnknownFields::Discard, limits())
        .err()
        .unwrap();
    assert_eq!(
        error.kind,
        capnp::ErrorKind::EnumValueOrUnionDiscriminantNotPresent(capnp::NotInSchema(60000))
    );
    assert!(
        error.extra.contains("field-api.capnp:Choice union"),
        "{error}"
    );
    Ok(())
}

#[test]
fn failed_conversion_releases_partial_capability_copies() -> capnp::Result<()> {
    let drops = Rc::new(Cell::new(0));
    let mut m = Message::<NativeRecord>::new()?;
    m.edit().cap().copy_from(client(&drops))?;
    m.edit().label().copy_from("label")?;
    assert!(m
        .read()
        .to_value(
            UnknownFields::Discard,
            Limits {
                bytes: 0,
                ..limits()
            }
        )
        .is_err());
    let value = m.read().to_value(UnknownFields::Discard, limits())?;
    assert!(value
        .to_message(Limits {
            bytes: 0,
            ..limits()
        })
        .is_err());
    drop(value);
    assert_eq!(drops.get(), 0);
    drop(m);
    assert_eq!(drops.get(), 1);
    Ok(())
}

#[derive(serde::Deserialize)]
struct Trace {
    steps: Vec<Step>,
}
#[derive(serde::Deserialize)]
struct Step {
    action: String,
    state: Vec<usize>,
}
#[test]
fn replay_tlc_native_value_traces() -> capnp::Result<()> {
    let path = reproto_test_support::verification::input("REPROTO_NATIVE_VALUE_TRACES")
        .expect("prepare verified trace corpus");
    let traces: Vec<Trace> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(!traces.is_empty());
    for trace in &traces {
        for far in [false, true] {
            let drops = Rc::new(Cell::new(0));
            let cap = client(&drops);
            let identity = cap.client.hook.get_ptr();
            let mut allocator = capnp::message::HeapAllocator::new();
            if far {
                allocator = allocator
                    .first_segment_words(1)
                    .allocation_strategy(capnp::message::AllocationStrategy::FixedSize);
            }
            let mut m = Message::<NativeRecord>::with_allocator(allocator)?;
            m.edit().cap().copy_from(cap)?;
            m.edit().label().copy_from("1")?;
            let mut source = Some(m);
            let mut value: Option<NativeRecordValue> = None;
            let mut output: Option<Message<NativeRecord>> = None;
            for step in &trace.steps {
                match step.action.as_str() {
                    "snapshot" => {
                        value = Some(
                            source
                                .as_ref()
                                .unwrap()
                                .read()
                                .to_value(UnknownFields::Discard, limits())?,
                        )
                    }
                    "fail-read" => {
                        let error = source
                            .as_ref()
                            .unwrap()
                            .read()
                            .to_value(
                                UnknownFields::Discard,
                                Limits {
                                    bytes: 0,
                                    ..limits()
                                },
                            )
                            .err()
                            .expect("read budget failure");
                        assert!(error.extra.contains("NativeRecord.label (@1)"), "{error}");
                        assert!(error.extra.starts_with("native decode "), "{error}");
                    }
                    "change" => source.as_mut().unwrap().edit().label().copy_from("2")?,
                    "encode" => output = Some(value.as_ref().unwrap().to_message(limits())?),
                    "fail-write" => {
                        let error = value
                            .as_ref()
                            .unwrap()
                            .to_message(Limits {
                                bytes: 0,
                                ..limits()
                            })
                            .err()
                            .expect("write budget failure");
                        assert!(error.extra.contains("NativeRecord.label (@1)"), "{error}");
                        assert!(error.extra.starts_with("native encode "), "{error}");
                    }
                    "drop-source" => {
                        source.take();
                    }
                    "drop-value" => {
                        value.take();
                    }
                    "drop-output" => {
                        output.take();
                    }
                    _ => panic!("unknown action"),
                }
                let state = &step.state;
                let parse = |text: &str| text.parse::<usize>().unwrap();
                assert_eq!(
                    source
                        .as_ref()
                        .map(|m| parse(m.read().label().unwrap()))
                        .unwrap_or(0),
                    state[0]
                );
                assert_eq!(
                    value
                        .as_ref()
                        .map(|v| parse(v.label.as_ref().unwrap()))
                        .unwrap_or(0),
                    state[1]
                );
                assert_eq!(
                    output
                        .as_ref()
                        .map(|m| parse(m.read().label().unwrap()))
                        .unwrap_or(0),
                    state[2]
                );
                assert_eq!(usize::from(drops.get() == 0), state[9]);
                assert!(drops.get() <= 1);
                if let Some(m) = &source {
                    assert_eq!(m.read().cap()?.client.hook.get_ptr(), identity);
                }
                if let Some(v) = &value {
                    assert_eq!(v.cap.as_ref().unwrap().client.hook.get_ptr(), identity);
                }
                if let Some(m) = &output {
                    assert_eq!(m.read().cap()?.client.hook.get_ptr(), identity);
                }
            }
            drop(source);
            drop(value);
            drop(output);
            assert_eq!(drops.get(), 1);
        }
    }
    Ok(())
}

#[test]
fn opaque_native_payload_retains_unknown_capabilities() -> capnp::Result<()> {
    let drops = Rc::new(Cell::new(0));
    let cap = client(&drops);
    let identity = cap.client.hook.get_ptr();
    let mut inner = Message::<Types>::new()?;
    inner.edit().cap().copy_from(cap)?;
    let mut outer = Message::<Types>::new()?;
    outer.edit().any().ensure()?.set_as::<Types>(inner.read())?;
    let native = outer.read().to_value(UnknownFields::Discard, limits())?;
    drop(inner);
    drop(outer);
    assert_eq!(drops.get(), 0);
    assert_eq!(
        native
            .any
            .as_ref()
            .unwrap()
            .read()?
            .get_as::<TypesRef<'_>>()?
            .cap()?
            .client
            .hook
            .get_ptr(),
        identity
    );
    let restored = native.to_message(limits())?;
    drop(native);
    assert_eq!(
        restored
            .read()
            .any()?
            .get_as::<TypesRef<'_>>()?
            .cap()?
            .client
            .hook
            .get_ptr(),
        identity
    );
    drop(restored);
    assert_eq!(drops.get(), 1);
    Ok(())
}
