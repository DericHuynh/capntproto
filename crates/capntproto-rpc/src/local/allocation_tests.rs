//! Liveness must not require a second owner or borrow mutable dispatch state.
use super::*;

#[test]
fn result_size_hints_preserve_default_root_and_preallocation_limit() {
    for (hint, expected_words) in [
        (None, 256),
        (Some(0), 1),
        (Some(255), 256),
        (Some(256), 257),
        (Some((1 << 20) - 1), 1 << 20),
        (Some(1 << 20), 1 << 20),
        (Some(u64::MAX), 1 << 20),
    ] {
        let mut message = message::Builder::new(result_allocator(hint.map(|word_count| {
            capnp::MessageSize {
                word_count,
                cap_count: u32::MAX,
            }
        })));
        message.initn_root::<capnp::data::Builder<'_>>(0);
        assert_eq!(
            message.retained_segment_bytes(),
            Some(expected_words * 8),
            "hint {hint:?}"
        );
    }
}

#[derive(Clone)]
struct Server(Rc<()>);
impl capability::Server for Server {
    fn dispatch_call(
        self,
        _: u64,
        _: u16,
        _: capability::CallContext<'_, any_pointer::Owned, any_pointer::Owned>,
    ) -> capability::DispatchCallResult {
        capability::DispatchCallResult::new(Promise::ok(()), false)
    }
    fn as_ptr(&self) -> usize {
        Rc::as_ptr(&self.0) as usize
    }
}

#[test]
fn local_client_construction_uses_one_owner_allocation() {
    let server = Server(Rc::new(()));
    let mut client = None;
    let allocations = allocation_counter::measure(|| {
        client = Some(Client::new(server));
    });
    assert_eq!(allocations.count_total, 1);
    assert!(client.is_some());
}

#[test]
fn registered_liveness_can_be_read_while_dispatch_state_is_borrowed() {
    let client = Client::new(Server(Rc::new(())));
    let registered: Rc<dyn RegisteredState> = client.state.clone();
    let exclusive = client.state.borrow_mut();
    assert!(registered.is_live());
    drop(exclusive);
    ClientState::revoke(&client.state, Error::failed("revoked".into()));
    assert!(!registered.is_live());
}

#[test]
fn local_identity_does_not_borrow_dispatch_state_or_allocate() {
    let client = Client::new(Server(Rc::new(())));
    let expected = client.get_ptr();
    let exclusive = client.state.borrow_mut();
    let counts = allocation_counter::measure(|| {
        assert_eq!(client.get_ptr(), expected);
        assert_eq!(
            client
                .as_any()
                .unwrap()
                .downcast_ref::<Identity>()
                .unwrap()
                .get(),
            expected
        );
        assert_eq!(client.clone().get_ptr(), expected);
    });
    assert_eq!(counts.count_total, 0);
    drop(exclusive);
}

#[test]
fn weak_registration_does_not_retain_the_service_or_its_state() {
    let token = Rc::new(());
    let client = Client::new(Server(token.clone()));
    let weak = Rc::downgrade(&client.state);
    let hook = client.into_hook();
    drop(hook);
    assert!(weak.upgrade().is_none());
    assert_eq!(Rc::strong_count(&token), 1);
}

#[test]
fn first_interface_registration_needs_only_the_returned_hook_allocation() {
    // Warm only the outer pointer table. A new server must not allocate an
    // inner dispatch-type map even on its first registration.
    drop(Client::new(Server(Rc::new(()))).into_hook());
    let client = Client::new(Server(Rc::new(())));
    let weak = Rc::downgrade(&client.state);
    let mut hook = None;
    let counts = allocation_counter::measure(|| {
        hook = Some(client.into_hook());
    });
    assert_eq!(counts.count_total, 1);
    assert!(weak.upgrade().is_some());
    drop(hook);
    assert!(weak.upgrade().is_none());
}

#[test]
fn interface_views_promote_survivors_and_old_owners_cannot_retire_replacements() {
    fn registration(client: &Client<Server>, kind: TypeId) -> LocalRegistration {
        let state: Rc<dyn RegisteredState> = client.state.clone();
        LocalRegistration {
            kind,
            state: Rc::downgrade(&state),
            revocable: false,
        }
    }
    let first = Client::new(Server(Rc::new(())));
    let second = Client::new(Server(Rc::new(())));
    let a = TypeId::of::<u8>();
    let b = TypeId::of::<u16>();
    let mut views = LocalViews {
        first: registration(&first, a),
        others: Vec::new(),
    };
    views.insert(registration(&second, b));
    assert!(!views.conflicts(false));
    assert!(views.conflicts(true));
    assert!(views.get(&a).unwrap().is_live());
    drop(first);
    assert!(!views.remove_dead(a));
    assert!(views.get(&a).is_none());
    assert!(views.get(&b).unwrap().is_live());
    let replacement = Client::new(Server(Rc::new(())));
    views.insert(registration(&replacement, b));
    drop(second);
    assert!(!views.remove_dead(b));
    assert!(views.get(&b).unwrap().is_live());
    drop(replacement);
    assert!(views.remove_dead(b));
}
