use capnp::{dynamic_orphan::ExternalData, dynamic_struct, dynamic_value as value};
use reproto_test_support::dynamic_test_capnp::external_case;
use std::sync::Arc;

fn words(len: usize) -> Arc<[capnp::Word]> {
    let mut words = capnp::Word::allocate_zeroed_vec(len.div_ceil(8));
    capnp::Word::words_to_bytes_mut(&mut words)[..len].fill(85);
    words.into()
}
fn allocator(small: bool) -> capnp::message::HeapAllocator {
    let a = capnp::message::HeapAllocator::new();
    if small {
        a.first_segment_words(1)
            .allocation_strategy(capnp::message::AllocationStrategy::FixedSize)
    } else {
        a
    }
}

#[test]
fn external_data_is_zero_copy_immutable_serializable_and_arena_owned() -> capnp::Result<()> {
    for small in [false, true] {
        for len in [0, 1, 8, 50, 4096] {
            let source = words(len);
            let address = source.as_ptr().cast::<u8>();
            let weak = Arc::downgrade(&source);
            let mut message = capnp::message::Builder::new(allocator(small));
            {
                let (mut root, token) =
                    value::Builder::from(message.init_root::<external_case::Builder>())
                        .downcast::<dynamic_struct::Builder>()
                        .with_orphanage();
                let mut orphan = token
                    .in_struct(&mut root)?
                    .reference_external_data(ExternalData::new(source, len)?)?;
                assert_eq!(weak.strong_count(), 1);
                for count in [0, u32::try_from(len).unwrap(), 8192] {
                    assert!(token
                        .in_struct(&mut root)?
                        .resize(&mut orphan, count)
                        .is_err());
                }
                assert!(token
                    .in_struct(&mut root)?
                    .edit(&mut orphan, |_| -> capnp::Result<()> {
                        panic!("immutable view escaped")
                    })
                    .is_err());
                let mut typed = orphan.release_as::<capnp::data::Owned>().unwrap();
                token.in_struct(&mut root)?.read_typed(&mut typed, |data| {
                    assert_eq!(data.as_ptr(), address);
                    assert_eq!(data, vec![85; len]);
                    Ok(())
                })?;
                assert!(token
                    .in_struct(&mut root)?
                    .edit_typed(&mut typed, |_| Ok(()))
                    .is_err());
                root.adopt_named("data", typed.into_dynamic()).unwrap();
                assert!(root.reborrow().get_named("data").is_err());
                let data = root
                    .reborrow_as_reader()
                    .get_named("data")?
                    .downcast::<capnp::data::Reader>();
                assert_eq!(data.as_ptr(), address);
            }
            let segments = message.get_segments_for_output();
            assert!(segments
                .iter()
                .any(|s| s.as_ptr() == address && s.len() == len.div_ceil(8) * 8));
            let serialized = capnp::serialize::write_message_to_words(&message);
            let read =
                capnp::serialize::read_message(&mut serialized.as_slice(), Default::default())?;
            assert_eq!(
                read.get_root::<external_case::Reader>()?.get_data()?,
                vec![85; len]
            );
            {
                let (mut root, token) =
                    value::Builder::from(message.get_root::<external_case::Builder>()?)
                        .downcast::<dynamic_struct::Builder>()
                        .with_orphanage();
                let orphan = root.disown_named("data", &token)?;
                root.adopt_named("other", orphan).unwrap();
                root.set_named("other", value::Reader::Data(b"replacement"))?;
                assert_eq!(weak.strong_count(), 1);
                assert_eq!(
                    &capnp::Word::words_to_bytes(&weak.upgrade().unwrap())[..len],
                    vec![85; len]
                );
            }
            drop(message);
            assert_eq!(weak.strong_count(), 0);
        }
    }
    Ok(())
}

#[test]
fn external_data_checks_padding_alignment_and_length_and_supports_owned_mappings(
) -> capnp::Result<()> {
    assert!(ExternalData::new(words(8), 9).is_err());
    assert!(ExternalData::new(words(8), 7).is_err());
    assert!(ExternalData::new(words(8), 1 << 29).is_err());
    let bytes = Arc::<[u8]>::from([1u8; 9]);
    // SAFETY: Arc keeps these immutable bytes at a stable address.
    assert!(unsafe { ExternalData::from_owner(bytes, 9) }.is_err());
    static EMPTY: [capnp::Word; 0] = [];
    ExternalData::from_static(&EMPTY, 0)?;
    let mut file = tempfile::tempfile().unwrap();
    std::io::Write::write_all(&mut file, &[85; 4096]).unwrap();
    // SAFETY: this private file is not modified until its mapping is dropped.
    let mapping = unsafe { memmap2::Mmap::map(&file) }.unwrap();
    let address = mapping.as_ptr();
    let mut message = capnp::message::Builder::new_default();
    let (mut root, token) = value::Builder::from(message.init_root::<external_case::Builder>())
        .downcast::<dynamic_struct::Builder>()
        .with_orphanage();
    // SAFETY: the read-only mapping retains its address across moves; the
    // private backing file stays unchanged for the entire arena lifetime.
    let data = unsafe { ExternalData::from_owner(mapping, 4096) }?;
    let orphan = token.in_struct(&mut root)?.reference_external_data(data)?;
    root.adopt_named("any", orphan).unwrap();
    let mut any = root
        .reborrow()
        .get_named("any")?
        .downcast::<capnp::any_pointer::Builder>();
    assert!(any.reborrow().get_as::<capnp::data::Builder>().is_err());
    assert!(any
        .reborrow()
        .get_as::<capnp::primitive_list::Builder<u8>>()
        .is_err());
    assert!(any
        .reborrow()
        .get_as::<capnp::struct_list::Builder<external_case::Owned>>()
        .is_err());
    assert_eq!(
        any.reborrow()
            .into_reader()
            .get_as::<capnp::data::Reader>()?
            .as_ptr(),
        address
    );
    any.clear();
    Ok(())
}

#[derive(serde::Deserialize)]
struct Step {
    action: String,
    state: Vec<u32>,
}
#[derive(serde::Deserialize)]
struct Case {
    steps: Vec<Step>,
}

struct Service(std::rc::Rc<std::cell::Cell<bool>>);
impl reproto_test_support::runtime_test_capnp::harness::Server for Service {}
impl Drop for Service {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

#[test]
fn replay_tlc_external_data_traces() -> capnp::Result<()> {
    use capnp::traits::ImbueMut;
    let path = reproto_test_support::verification::input("REPROTO_EXTERNAL_DATA_TRACES")
        .expect("prepare verified trace corpus");
    let cases: Vec<Case> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    for small in [false, true] {
        for case in &cases {
            let mut source = Some(words(50));
            let weak = Arc::downgrade(source.as_ref().unwrap());
            let address = source.as_ref().unwrap().as_ptr().cast::<u8>();
            let alive = std::rc::Rc::new(std::cell::Cell::new(true));
            let mut caps = vec![];
            let mut message = capnp::message::Builder::new(allocator(small));
            let mut closing = None;
            let mut state = vec![0, 1, 0, 1, 0, 0, 0, 0, 0, 0, 85, 0, 1];
            {
                let mut root = message.init_root::<external_case::Builder>();
                root.imbue_mut(&mut caps);
                root.set_token(capnp_rpc::new_client(Service(alive.clone())));
                let (mut root, token) = value::Builder::from(root)
                    .downcast::<dynamic_struct::Builder>()
                    .with_orphanage();
                let mut orphan = None;
                for (index, step) in case.steps.iter().enumerate() {
                    match step.action.as_str() {
                        "allocate" => {
                            orphan = Some(token.in_struct(&mut root)?.reference_external_data(
                                ExternalData::new(source.as_ref().unwrap().clone(), 50)?,
                            )?);
                            state[0] = 1;
                            state[11] = 1;
                        }
                        "source" => {
                            source.take();
                        }
                        "read" => {
                            state[4] = 1;
                        }
                        "edit" => {
                            if let Some(o) = &mut orphan {
                                assert!(token.in_struct(&mut root)?.edit(o, |_| Ok(())).is_err());
                            } else {
                                assert!(root.reborrow().get_named("data").is_err());
                            }
                            state[5] = 1;
                        }
                        "resize" => {
                            for count in [0, 50, 80] {
                                assert!(token
                                    .in_struct(&mut root)?
                                    .resize(orphan.as_mut().unwrap(), count)
                                    .is_err());
                            }
                            state[6] = 1;
                        }
                        "copy" => {
                            let mut copy_message = capnp::message::Builder::new_default();
                            let (mut copy_root, copy_token) = value::Builder::from(
                                copy_message.init_root::<external_case::Builder>(),
                            )
                            .downcast::<dynamic_struct::Builder>()
                            .with_orphanage();
                            let mut copied = if let Some(o) = &mut orphan {
                                token
                                    .in_struct(&mut root)?
                                    .read(o, |v| copy_token.in_struct(&mut copy_root)?.copy(v))?
                            } else {
                                copy_token
                                    .in_struct(&mut copy_root)?
                                    .copy(root.reborrow_as_reader().get_named("data")?)?
                            };
                            copy_token
                                .in_struct(&mut copy_root)?
                                .edit(&mut copied, |v| {
                                    let data = v.downcast::<capnp::data::Builder>();
                                    assert_ne!(data.as_ptr(), address);
                                    data.fill(170);
                                    Ok(())
                                })?;
                            state[7] = 1;
                        }
                        "reject" => {
                            let error = root
                                .adopt_named("token", orphan.take().unwrap())
                                .unwrap_err();
                            assert_eq!(error.error.kind, capnp::ErrorKind::TypeMismatch);
                            let mut other = capnp::message::Builder::new_default();
                            let mut other =
                                value::Builder::from(other.init_root::<external_case::Builder>())
                                    .downcast::<dynamic_struct::Builder>();
                            let error = other.adopt_named("data", error.orphan).unwrap_err();
                            assert_eq!(error.error.kind, capnp::ErrorKind::WrongArena);
                            orphan = Some(error.orphan);
                            state[8] = 1;
                        }
                        "adopt" => {
                            root.adopt_named("data", orphan.take().unwrap()).unwrap();
                            state[0] = 2;
                        }
                        "disown" => {
                            orphan = Some(root.disown_named("data", &token)?);
                            state[0] = 1;
                            state[9] = 1;
                        }
                        "drop" => {
                            if orphan.take().is_none() {
                                root.clear_named("data")?;
                            }
                            state[0] = 3;
                        }
                        "close" => {
                            assert_eq!(index + 1, case.steps.len());
                            closing = Some(step);
                            break;
                        }
                        other => panic!("unknown action {other}"),
                    }
                    if let Some(o) = &mut orphan {
                        token.in_struct(&mut root)?.read(o, |v| {
                            let data = v.downcast::<capnp::data::Reader>();
                            assert_eq!(data.as_ptr(), address);
                            assert_eq!(data, &[85; 50]);
                            Ok(())
                        })?;
                    } else if state[0] == 2 {
                        let data = root
                            .reborrow_as_reader()
                            .get_named("data")?
                            .downcast::<capnp::data::Reader>();
                        assert_eq!(data.as_ptr(), address);
                        assert_eq!(data, &[85; 50]);
                    }
                    if let Some(bytes) = weak.upgrade() {
                        assert_eq!(&capnp::Word::words_to_bytes(&bytes)[..50], &[85; 50]);
                    }
                    state[1] = u32::from(source.is_some());
                    state[3] = u32::try_from(weak.strong_count()).unwrap();
                    state[2] = state[3] - state[1];
                    state[12] = u32::from(alive.get());
                    assert_eq!(state, step.state, "{} small={small}", step.action);
                }
            }
            if let Some(step) = closing {
                drop(message);
                drop(caps);
                state[0] = 4;
                state[2] = 0;
                state[3] = u32::try_from(weak.strong_count()).unwrap();
                state[12] = u32::from(alive.get());
                assert_eq!(state, step.state);
            }
        }
    }
    Ok(())
}
