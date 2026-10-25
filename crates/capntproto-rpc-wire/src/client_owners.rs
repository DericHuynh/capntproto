//! The legacy client and request types share a caller-backed capability lease.
use crate::rpc_capnp::{self, api};
use capnp::{
    capability::{CallHints, Client, ClientStorage, PayloadStorage, Request, RequestStorage},
    field_api::{FixedField, FixedStruct, Struct},
    message, Error, ErrorKind, MessageSize,
};
use core::cell::{Cell, RefCell, RefMut};
const ZERO: capnp::Word = capnp::word(0, 0, 0, 0, 0, 0, 0, 0);
type Params = rpc_capnp::exception::Owned;

struct Connection<'buffer> {
    message: RefCell<message::FixedBuilder<'buffer>>,
    next_id: Cell<u32>,
    full: Cell<bool>,
    generation: Cell<u32>,
}
impl<'buffer> Connection<'buffer> {
    fn new(words: &'buffer mut [capnp::Word]) -> Self {
        let mut message = message::FixedBuilder::new(words).unwrap();
        let root = message.try_init_root::<api::Message>().unwrap();
        let mut call = root.into_field(api::Message::CALL).init().unwrap();
        call.scalar(api::Call::QUESTION_ID).set(u32::MAX);
        call.into_field(api::Call::PARAMS).init().unwrap();
        Self {
            message: RefCell::new(message),
            next_id: Cell::new(37),
            full: Cell::new(false),
            generation: Cell::new(1),
        }
    }
}
#[derive(Clone, Copy)]
struct Capability<'context, 'buffer> {
    connection: &'context Connection<'buffer>,
    generation: u32,
}
struct Draft<'call, 'buffer> {
    message: RefMut<'call, message::FixedBuilder<'buffer>>,
    connection: &'call Connection<'buffer>,
    generation: u32,
}
impl<'buffer> ClientStorage<Params, Params> for Capability<'_, 'buffer> {
    type Request<'call>
        = Draft<'call, 'buffer>
    where
        Self: 'call;
    fn try_new_call(
        &self,
        interface_id: u64,
        method_id: u16,
        _: Option<MessageSize>,
        hints: CallHints,
    ) -> capnp::Result<Self::Request<'_>> {
        if self.generation != self.connection.generation.get() {
            return Err(Error::from_kind(ErrorKind::Disconnected));
        }
        let mut message = self
            .connection
            .message
            .try_borrow_mut()
            .map_err(|_| Error::from_kind(ErrorKind::Overloaded))?;
        let root = message.try_upgrade_root::<api::Message>()?;
        let mut call = root.into_field(api::Message::CALL).upgrade()?;
        call.scalar(api::Call::INTERFACE_ID).set(interface_id);
        call.scalar(api::Call::METHOD_ID).set(method_id);
        call.scalar(api::Call::NO_PROMISE_PIPELINING)
            .set(hints.no_promise_pipelining);
        call.scalar(api::Call::ONLY_PROMISE_PIPELINE)
            .set(hints.only_promise_pipeline);
        Ok(Draft {
            message,
            connection: self.connection,
            generation: self.generation,
        })
    }
}
impl Draft<'_, '_> {
    fn content(&mut self) -> capnp::Result<FixedField<'_, Struct<api::Exception>>> {
        let root = self.message.try_upgrade_root::<api::Message>()?;
        Ok(root
            .into_field(api::Message::CALL)
            .upgrade()?
            .into_field(api::Call::PARAMS)
            .upgrade()?
            .into_field(api::Payload::CONTENT)
            .cast())
    }
}
impl PayloadStorage<Params> for Draft<'_, '_> {
    type Builder<'a>
        = FixedStruct<'a, api::Exception>
    where
        Self: 'a;
    fn try_get(&mut self, _: Option<MessageSize>) -> capnp::Result<Self::Builder<'_>> {
        let mut content = self.content()?;
        if content.is_null()? {
            content.init()
        } else {
            content.upgrade()
        }
    }
    fn try_init(&mut self, _: Option<MessageSize>) -> capnp::Result<Self::Builder<'_>> {
        self.content()?.replace()
    }
}
impl<'call, 'buffer> RequestStorage<Params> for Draft<'call, 'buffer> {
    type Sent = Result<RefMut<'call, message::FixedBuilder<'buffer>>, Self>;
    fn publish(mut self) -> Self::Sent {
        if self.connection.full.get() || self.generation != self.connection.generation.get() {
            return Err(self);
        }
        let id = self.connection.next_id.get();
        self.message
            .try_upgrade_root::<api::Message>()
            .unwrap()
            .into_field(api::Message::CALL)
            .upgrade()
            .unwrap()
            .scalar(api::Call::QUESTION_ID)
            .set(id);
        self.connection.next_id.set(id + 1);
        Ok(self.message)
    }
}
fn request<H: ClientStorage<Params, Params>>(
    client: &Client<H>,
) -> Request<Params, Params, H::Request<'_>> {
    client
        .try_new_call(
            42,
            3,
            None,
            CallHints {
                no_promise_pipelining: true,
                only_promise_pipeline: false,
            },
        )
        .unwrap()
}

#[test]
fn cloned_borrowed_client_keeps_one_request_guard_and_preserves_wire_headers() {
    let measured = allocation_counter::measure(|| {
        let mut words = [ZERO; 32];
        let connection = Connection::new(&mut words);
        let capability = Capability {
            connection: &connection,
            generation: 1,
        };
        let client = Client::from_owner(&capability);
        let alias = client.clone();
        let mut call = request(&client);
        assert!(alias
            .try_new_call::<Params, Params>(42, 3, None, CallHints::default())
            .is_err());
        assert_eq!(connection.next_id.get(), 37);
        call.try_get()
            .unwrap()
            .field(api::Exception::REASON)
            .copy_from("saved")
            .unwrap();
        assert!(call.try_get().unwrap().field(api::Exception::REASON).copy_from("This parameter is deliberately too long for this fixed request buffer. Its failure must preserve the original content and header, without allocating another buffer or assigning a wire question ID.").is_err());
        let Ok(output) = call.send() else {
            panic!("publication rejected")
        };
        assert_eq!(connection.next_id.get(), 38);
        let root = output
            .get_root_as_reader::<rpc_capnp::message::Reader<'_>>()
            .unwrap();
        let rpc_capnp::message::Call(call) = root.which().unwrap() else {
            panic!("Call lost")
        };
        let call = call.unwrap();
        assert_eq!(
            (
                call.get_question_id(),
                call.get_interface_id(),
                call.get_method_id()
            ),
            (37, 42, 3)
        );
        assert!(call.get_no_promise_pipelining());
        assert!(!call.get_only_promise_pipeline());
        assert_eq!(
            call.get_params()
                .unwrap()
                .get_content()
                .get_as::<rpc_capnp::exception::Reader<'_>>()
                .unwrap()
                .get_reason()
                .unwrap(),
            "saved"
        );
        drop(output);
        drop(request(&alias));
        assert_eq!(connection.next_id.get(), 38);
    });
    assert_eq!((measured.count_total, measured.bytes_total), (0, 0));
}

#[test]
fn full_publication_returns_original_owner_for_retry_without_consuming_id() {
    let measured = allocation_counter::measure(|| {
        let mut words = [ZERO; 32];
        let connection = Connection::new(&mut words);
        let client = Client::from_owner(Capability {
            connection: &connection,
            generation: 1,
        });
        let mut call = request(&client);
        call.try_get()
            .unwrap()
            .field(api::Exception::REASON)
            .copy_from("retry")
            .unwrap();
        let remaining = call.hook.message.remaining_words();
        connection.full.set(true);
        let Err(owner) = call.send() else {
            panic!("full output accepted")
        };
        assert_eq!(connection.next_id.get(), 37);
        assert_eq!(owner.message.remaining_words(), remaining);
        assert!(connection.message.try_borrow_mut().is_err());
        connection.full.set(false);
        let Ok(output) = Request::<Params, Params, _>::from_owner(owner).send() else {
            panic!("retry rejected")
        };
        assert_eq!(connection.next_id.get(), 38);
        drop(output);
        assert!(connection.message.try_borrow_mut().is_ok());
    });
    assert_eq!((measured.count_total, measured.bytes_total), (0, 0));
}

#[test]
fn revoked_generation_rejects_new_and_already_prepared_calls() {
    let measured = allocation_counter::measure(|| {
        let mut words = [ZERO; 32];
        let connection = Connection::new(&mut words);
        let client = Client::from_owner(Capability {
            connection: &connection,
            generation: 1,
        });
        let call = request(&client);
        connection.generation.set(2);
        let Err(owner) = call.send() else {
            panic!("revoked publication accepted")
        };
        assert_eq!(connection.next_id.get(), 37);
        drop(owner);
        assert!(client
            .try_new_call::<Params, Params>(42, 3, None, CallHints::default())
            .is_err());
        let replacement = Client::from_owner(Capability {
            connection: &connection,
            generation: 2,
        });
        let Ok(output) = request(&replacement).send() else {
            panic!("replacement rejected")
        };
        drop(output);
        assert_eq!(connection.next_id.get(), 38);
        let owner = replacement.into_owner();
        assert_eq!(owner.generation, 2);
    });
    assert_eq!((measured.count_total, measured.bytes_total), (0, 0));
}
