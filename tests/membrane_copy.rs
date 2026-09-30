use capnp::{
    any_pointer,
    capability::{Client, FromClientHook},
    dynamic_orphan::{Orphan, Orphanage, Root},
    dynamic_value,
    introspect::Introspect,
    message,
    private::layout::CapTable,
    traits::{Imbue, ImbueMut},
    Error,
};
use capnp_rpc::membrane::{Direction, Membrane, Policy};
use reproto_test_support::membrane_copy_capnp::{empty, payload, service};
use std::{cell::Cell, rc::Rc};

mod membrane_copy {
    pub mod verification;
}

#[derive(Default)]
struct Stats {
    live: Cell<u64>,
    calls: Cell<u64>,
    direction: Cell<u64>,
}
struct Server(Rc<Stats>);
impl Drop for Server {
    fn drop(&mut self) {
        self.0.live.set(self.0.live.get() - 1);
    }
}
impl service::Server for Server {
    async fn ping(
        self: Rc<Self>,
        _: service::PingParams,
        mut results: service::PingResults,
    ) -> capnp::Result<()> {
        self.0.calls.set(self.0.calls.get() + 1);
        results.get().set_value(17);
        Ok(())
    }
    async fn bounce(
        self: Rc<Self>,
        params: service::BounceParams,
        mut results: service::BounceResults,
    ) -> capnp::Result<()> {
        results.get().set_cap(params.get()?.get_cap()?);
        Ok(())
    }
}
fn server(stats: &Rc<Stats>) -> service::Client {
    stats.live.set(stats.live.get() + 1);
    capnp_rpc::new_client(Server(stats.clone()))
}
struct Boundary(Rc<Stats>);
impl Policy for Boundary {
    fn call(
        &self,
        direction: Direction,
        _: u64,
        _: u16,
        _: &Client,
    ) -> capnp::Result<Option<Client>> {
        self.0.direction.set(if direction == Direction::Inbound {
            1
        } else {
            2
        });
        Ok(None)
    }
}

#[derive(Default)]
struct Message {
    data: message::Builder<message::HeapAllocator>,
    caps: CapTable,
}
impl Message {
    fn reader(&self) -> any_pointer::Reader<'_> {
        let mut reader = self
            .data
            .get_root_as_reader::<any_pointer::Reader>()
            .unwrap();
        reader.imbue(&self.caps);
        reader
    }
    fn builder(&mut self) -> any_pointer::Builder<'_> {
        let mut builder = self.data.get_root::<any_pointer::Builder>().unwrap();
        builder.imbue_mut(&mut self.caps);
        builder
    }
    fn root(&mut self) -> (Root<'_>, Orphanage<'_>) {
        Root::new(self.builder(), any_pointer::Owned::introspect())
            .unwrap()
            .with_orphanage()
    }
    fn source(form: u64, cap: service::Client) -> Self {
        let mut message = Self::default();
        match form {
            0 | 2 => fill(message.builder().init_as::<payload::Builder>(), &cap),
            1 => {
                let mut list = message
                    .builder()
                    .initn_as::<capnp::struct_list::Builder<payload::Owned>>(2);
                for i in 0..2 {
                    fill(list.reborrow().get(i), &cap);
                }
            }
            3 => {
                let mut list = message
                    .builder()
                    .initn_as::<capnp::capability_list::Builder<service::Client>>(2);
                list.set(0, cap.clone().into_client_hook());
                list.set(1, cap.into_client_hook());
            }
            _ => panic!(),
        }
        message
    }
}
fn fill(mut value: payload::Builder<'_>, cap: &service::Client) {
    value.set_number(73);
    value.set_cap(cap.clone());
    value.set_other(cap.clone());
    let mut caps = value.reborrow().init_caps(2);
    caps.set(0, cap.clone().into_client_hook());
    caps.set(1, cap.clone().into_client_hook());
    let mut nested = value.reborrow().init_nested(1).get(0);
    nested.set_number(91);
    nested.set_cap(cap.clone());
    value
        .init_opaque()
        .set_as_capability(cap.clone().into_client_hook());
}
fn read_caps(form: u64, reader: any_pointer::Reader<'_>) -> capnp::Result<Vec<service::Client>> {
    fn from_struct(
        value: payload::Reader<'_>,
        caps: &mut Vec<service::Client>,
    ) -> capnp::Result<()> {
        assert_eq!(value.get_number(), 73);
        caps.push(value.get_cap()?);
        caps.push(value.get_other()?);
        for cap in value.get_caps()?.iter() {
            caps.push(cap?);
        }
        let child = value.get_nested()?.get(0);
        assert_eq!(child.get_number(), 91);
        caps.push(child.get_cap()?);
        caps.push(value.get_opaque().get_as_capability()?);
        Ok(())
    }
    let mut caps = Vec::new();
    match form {
        0 | 2 => from_struct(reader.get_as()?, &mut caps)?,
        1 => {
            for value in reader
                .get_as::<capnp::struct_list::Reader<payload::Owned>>()?
                .iter()
            {
                from_struct(value, &mut caps)?;
            }
        }
        3 => {
            for cap in reader
                .get_as::<capnp::capability_list::Reader<service::Client>>()?
                .iter()
            {
                caps.push(cap?);
            }
        }
        _ => panic!(),
    }
    Ok(caps)
}
fn copied<'m>(
    form: u64,
    membrane: &Membrane,
    inward: bool,
    reader: any_pointer::Reader<'_>,
    to: &mut capnp::dynamic_orphan::Access<'_, 'm>,
) -> capnp::Result<Orphan<'m>> {
    let value = match form {
        0 => reader.get_as::<payload::Reader>()?.into(),
        1 => reader
            .get_as::<capnp::struct_list::Reader<payload::Owned>>()?
            .into(),
        2 => dynamic_value::Reader::AnyPointer(reader),
        3 => reader
            .get_as::<capnp::capability_list::Reader<service::Client>>()?
            .into(),
        _ => panic!(),
    };
    if inward {
        membrane.copy_into(value, to)
    } else {
        membrane.copy_out(value, to)
    }
}
fn root_reader<'a>(root: &'a Root<'_>) -> any_pointer::Reader<'a> {
    let dynamic_value::Reader::AnyPointer(reader) = root.as_reader().unwrap() else {
        panic!()
    };
    reader
}
fn orphan_caps(
    form: u64,
    root: &mut Root<'_>,
    token: &Orphanage<'_>,
    orphan: &mut Orphan<'_>,
) -> Vec<service::Client> {
    token
        .in_root(root)
        .unwrap()
        .read(orphan, |value| {
            match value {
                dynamic_value::Reader::AnyPointer(reader) => read_caps(form, reader),
                // Copying through a native reader preserves the native orphan type.
                other => {
                    let mut temporary = Message::default();
                    // This is only an observation; copying the already transformed
                    // hooks here must not apply the membrane a second time.
                    match other {
                        dynamic_value::Reader::Struct(value) => {
                            temporary
                                .builder()
                                .set_as::<payload::Owned>(value.downcast::<payload::Owned>())?
                        }
                        dynamic_value::Reader::List(value) if form == 1 => {
                            temporary
                                .builder()
                                .set_as::<capnp::struct_list::Owned<payload::Owned>>(
                                    dynamic_value::Reader::List(value)
                                        .downcast::<capnp::struct_list::Reader<payload::Owned>>(),
                                )?
                        }
                        dynamic_value::Reader::List(value) => {
                            temporary
                                .builder()
                                .set_as::<capnp::capability_list::Owned<service::Client>>(
                                dynamic_value::Reader::List(value)
                                    .downcast::<capnp::capability_list::Reader<service::Client>>(),
                            )?
                        }
                        _ => panic!(),
                    }
                    read_caps(form, temporary.reader())
                }
            }
        })
        .unwrap()
}
fn observe(caps: Vec<service::Client>, stats: &Stats, original: usize) -> u64 {
    if caps.is_empty() {
        return 0;
    }
    let first = caps[0].as_client_hook().get_ptr();
    stats.direction.set(0);
    let answer = futures::executor::block_on(caps[0].ping_request().send().promise);
    match answer {
        Ok(value) => {
            // Fresh broken wrappers after revocation need not be cached.
            assert!(caps
                .iter()
                .all(|cap| cap.as_client_hook().get_ptr() == first));
            assert_eq!(value.get().unwrap().get_value(), 17);
            if first == original {
                assert_eq!(stats.direction.get(), 0);
                3
            } else {
                let direction = stats.direction.get();
                assert!(direction == 1 || direction == 2);
                direction
            }
        }
        Err(error) => {
            assert!(error.to_string().contains("revoked"), "{error}");
            4
        }
    }
}

#[test]
fn copying_does_not_depend_on_source_lifetime_and_reverse_crossings_unwrap() {
    for form in 0..4 {
        for inward in [false, true] {
            let stats = Rc::new(Stats::default());
            let cap = server(&stats);
            let original = cap.as_client_hook().get_ptr();
            let source = Message::source(form, cap);
            let membrane = Membrane::new(Rc::new(Boundary(stats.clone())));
            let mut target = Message::default();
            let (mut root, token) = target.root();
            let mut orphan = copied(
                form,
                &membrane,
                inward,
                source.reader(),
                &mut token.in_root(&mut root).unwrap(),
            )
            .unwrap();
            drop(source);
            assert!(root.is_null());
            assert_eq!(stats.calls.get(), 0);
            assert_eq!(
                observe(
                    orphan_caps(form, &mut root, &token, &mut orphan),
                    &stats,
                    original
                ),
                if inward { 2 } else { 1 }
            );
            root.adopt(orphan).unwrap();
            let mut back = Message::default();
            let (mut back_root, back_token) = back.root();
            let orphan = copied(
                form,
                &membrane,
                !inward,
                root_reader(&root),
                &mut back_token.in_root(&mut back_root).unwrap(),
            )
            .unwrap();
            back_root.adopt(orphan).unwrap();
            assert_eq!(
                observe(
                    read_caps(form, root_reader(&back_root)).unwrap(),
                    &stats,
                    original
                ),
                3
            );
            membrane.revoke(Error::failed("revoked".into()));
            assert_eq!(
                observe(
                    read_caps(form, root_reader(&root)).unwrap(),
                    &stats,
                    original
                ),
                4
            );
            assert_eq!(
                observe(
                    read_caps(form, root_reader(&back_root)).unwrap(),
                    &stats,
                    original
                ),
                3
            );
            root.clear();
            back_root.clear();
            assert_eq!(stats.live.get(), 0);
        }
    }
}

#[test]
fn unknown_fields_and_orphan_adoption_failures_preserve_authority() {
    let stats = Rc::new(Stats::default());
    let cap = server(&stats);
    let original = cap.as_client_hook().get_ptr();
    let source = Message::source(0, cap);
    let membrane = Membrane::new(Rc::new(Boundary(stats.clone())));
    let mut target = Message::default();
    let (mut root, token) = target.root();
    let orphan = membrane
        .copy_out(
            source.reader().get_as::<empty::Reader>().unwrap(),
            &mut token.in_root(&mut root).unwrap(),
        )
        .unwrap();
    // The schema is empty, but the unknown capability/data fields survive.
    let mut foreign = Message::default();
    let (mut wrong, _) = foreign.root();
    let rejected = wrong.adopt(orphan).unwrap_err();
    assert_eq!(rejected.error.kind, capnp::ErrorKind::WrongArena);
    assert!(wrong.is_null());
    root.adopt(rejected.orphan).unwrap();
    assert_eq!(
        observe(read_caps(0, root_reader(&root)).unwrap(), &stats, original),
        1
    );
    drop(source);
    root.clear();
    assert_eq!(stats.live.get(), 0);
}

#[test]
fn failed_or_panicking_transforms_release_the_unpublished_copy() {
    for panic in [false, true] {
        let stats = Rc::new(Stats::default());
        let cap = server(&stats);
        let source = Message::source(0, cap);
        let mut target = Message::default();
        target
            .builder()
            .set_as::<capnp::text::Owned>("retained")
            .unwrap();
        let (mut root, token) = target.root();
        let mut callbacks = 0;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            token
                .in_root(&mut root)
                .unwrap()
                .copy_with_capability_transform(source.reader().into(), |cap| {
                    callbacks += 1;
                    if callbacks == 2 {
                        if panic {
                            panic!("injected transform failure");
                        }
                        return Err(Error::failed("injected transform failure".into()));
                    }
                    Ok(cap)
                })
                .map(|_| panic!("injected failure accepted"))
        }));
        if panic {
            assert!(result.is_err());
        } else {
            assert!(result.unwrap().is_err());
        }
        assert_eq!(
            root_reader(&root)
                .get_as::<capnp::text::Reader>()
                .unwrap()
                .to_str()
                .unwrap(),
            "retained"
        );
        drop(source);
        assert_eq!(stats.live.get(), 0, "copy retained authority after failure");
    }
}

#[test]
fn malformed_copies_fail_before_policy_callbacks_and_preserve_existing_values() {
    let stats = Rc::new(Stats::default());
    let caps = vec![Some(server(&stats).into_client_hook())];
    let words = [
        capnp::word(0, 0, 0, 0, 0, 0, 2, 0),
        capnp::word(3, 0, 0, 0, 0, 0, 0, 0),
        capnp::word(7, 0, 0, 0, 0, 0, 0, 0),
    ];
    let segments = [capnp::Word::words_to_bytes(&words)];
    let source = message::Reader::new(&segments[..], message::ReaderOptions::new());
    let mut reader = source.get_root::<any_pointer::Reader>().unwrap();
    reader.imbue(&caps);
    let mut destination = Message::default();
    destination
        .builder()
        .set_as::<capnp::text::Owned>("unchanged")
        .unwrap();
    let (mut root, token) = destination.root();
    let mut callbacks = 0;
    let result = token
        .in_root(&mut root)
        .unwrap()
        .copy_with_capability_transform(reader.into(), |cap| {
            callbacks += 1;
            Ok(cap)
        });
    assert!(result.is_err());
    assert_eq!(callbacks, 0);
    assert_eq!(
        root_reader(&root)
            .get_as::<capnp::text::Reader>()
            .unwrap()
            .to_str()
            .unwrap(),
        "unchanged"
    );
    drop(caps);
    assert_eq!(
        stats.live.get(),
        0,
        "failed deep copy retained a source capability"
    );
}

#[test]
fn copying_promises_does_not_poll_resolution() {
    let stats = Rc::new(Stats::default());
    let polls = Rc::new(Cell::new(0));
    let count = polls.clone();
    let service_stats = stats.clone();
    let cap: service::Client = capnp_rpc::new_future_client(async move {
        count.set(count.get() + 1);
        Ok(server(&service_stats))
    });
    let source = Message::source(0, cap);
    let boundary = Membrane::new(Rc::new(Boundary(stats.clone())));
    let mut destination = Message::default();
    let (mut root, token) = destination.root();
    let orphan = boundary
        .copy_out(source.reader(), &mut token.in_root(&mut root).unwrap())
        .unwrap();
    drop(source);
    assert_eq!(polls.get(), 0);
    assert_eq!(stats.live.get(), 0);
    root.adopt(orphan).unwrap();
    let cap = read_caps(0, root_reader(&root)).unwrap().remove(0);
    let result = futures::executor::block_on(cap.ping_request().send().promise).unwrap();
    assert_eq!(result.get().unwrap().get_value(), 17);
    assert_eq!(polls.get(), 1);
    assert_eq!(stats.direction.get(), 1);
    drop(result);
    drop(cap);
    root.clear();
    assert_eq!(stats.live.get(), 0);
}

#[test]
fn substitutions_errors_and_data_only_copies_follow_membrane_rules() {
    struct Substitute {
        replacement: service::Client,
        imports: Cell<u32>,
        exports: Cell<u32>,
    }
    impl Policy for Substitute {
        fn call(&self, _: Direction, _: u64, _: u16, _: &Client) -> capnp::Result<Option<Client>> {
            panic!("substitution invoked boundary call policy")
        }
        fn import_external(&self, _: &Client) -> capnp::Result<Option<Client>> {
            self.imports.set(self.imports.get() + 1);
            Ok(Some(self.replacement.client.clone()))
        }
        fn export_internal(&self, _: &Client) -> capnp::Result<Option<Client>> {
            self.exports.set(self.exports.get() + 1);
            Err(Error::failed("export denied".into()))
        }
    }
    let source_stats = Rc::new(Stats::default());
    let replacement_stats = Rc::new(Stats::default());
    let policy = Rc::new(Substitute {
        replacement: server(&replacement_stats),
        imports: Cell::new(0),
        exports: Cell::new(0),
    });
    let membrane = Membrane::new(policy.clone());
    let source = Message::source(0, server(&source_stats));
    let mut destination = Message::default();
    let (mut root, token) = destination.root();
    let orphan = membrane
        .copy_into(source.reader(), &mut token.in_root(&mut root).unwrap())
        .unwrap();
    root.adopt(orphan).unwrap();
    assert!(policy.imports.get() > 0);
    assert_eq!(policy.exports.get(), 0);
    membrane.revoke(Error::failed("revoked".into()));
    // Substitutions are deliberately outside this membrane's revocation rules.
    let caps = read_caps(0, root_reader(&root)).unwrap();
    let answer = futures::executor::block_on(caps[0].ping_request().send().promise).unwrap();
    assert_eq!(answer.get().unwrap().get_value(), 17);
    drop(answer);
    drop(caps);
    assert_eq!(source_stats.calls.get(), 0);
    assert_eq!(replacement_stats.calls.get(), 1);
    let orphan = membrane
        .copy_out(source.reader(), &mut token.in_root(&mut root).unwrap())
        .unwrap();
    root.adopt(orphan).unwrap();
    assert!(policy.exports.get() > 0);
    for cap in read_caps(0, root_reader(&root)).unwrap() {
        let error = futures::executor::block_on(cap.ping_request().send().promise)
            .err()
            .unwrap();
        assert!(error.to_string().contains("export denied"));
    }
    let before = (policy.imports.get(), policy.exports.get());
    for inward in [false, true] {
        let input = dynamic_value::Reader::Text("data survives revocation".into());
        let orphan = if inward {
            membrane.copy_into(input, &mut token.in_root(&mut root).unwrap())
        } else {
            membrane.copy_out(input, &mut token.in_root(&mut root).unwrap())
        }
        .unwrap();
        root.adopt(orphan).unwrap();
        assert_eq!(
            root_reader(&root)
                .get_as::<capnp::text::Reader>()
                .unwrap()
                .to_str()
                .unwrap(),
            "data survives revocation"
        );
    }
    assert_eq!((policy.imports.get(), policy.exports.get()), before);
    root.clear();
    drop(source);
    assert_eq!(source_stats.live.get(), 0);
    drop(membrane);
    drop(policy);
    assert_eq!(replacement_stats.live.get(), 0);
}

#[test]
fn inline_groups_transform_nested_capabilities_and_exclude_parent_siblings() {
    use reproto_test_support::membrane_copy_capnp::grouped;
    let stats = Rc::new(Stats::default());
    let sibling_stats = Rc::new(Stats::default());
    let cap = server(&stats);
    let original = cap.as_client_hook().get_ptr();
    let mut source = Message::default();
    let mut fields = source.builder().init_as::<grouped::Builder>();
    fields.set_outside(server(&sibling_stats));
    let mut body = fields.init_body();
    body.set_count(42);
    body.set_cap(cap.clone());
    body.init_nested().set_other(cap);
    let membrane = Membrane::new(Rc::new(Boundary(stats.clone())));
    let mut destination = Message::default();
    let (mut root, token) = destination.root();
    let mut orphan = membrane
        .copy_out(
            source
                .reader()
                .get_as::<grouped::Reader>()
                .unwrap()
                .get_body(),
            &mut token.in_root(&mut root).unwrap(),
        )
        .unwrap();
    drop(source);
    assert_eq!(
        sibling_stats.live.get(),
        0,
        "group copy retained parent sibling"
    );
    let caps = token
        .in_root(&mut root)
        .unwrap()
        .read_group(&mut orphan, |mut group| {
            group.read_named("count", |value| {
                assert_eq!(value.downcast::<u32>(), 42);
                Ok(())
            })?;
            let cap = group.read_named("cap", |value| {
                value
                    .downcast::<dynamic_value::Capability>()
                    .cast::<service::Client>()
            })?;
            let other = group.read_group_named("nested", |mut nested| {
                nested.read_named("other", |value| {
                    value
                        .downcast::<dynamic_value::Capability>()
                        .cast::<service::Client>()
                })
            })?;
            Ok(vec![cap, other])
        })
        .unwrap();
    assert_eq!(observe(caps, &stats, original), 1);
    drop(orphan);
    assert_eq!(stats.live.get(), 0);
}
