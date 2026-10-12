//! Bounded cleanup draining: failed output preserves the ledger's pending work.
use super::*;
use capnp::field_api::Schema;
use capntproto_rpc_engine::questions;

pub(crate) fn abort_reason(
    exception: rpc_capnp::exception::Reader<'_>,
) -> Result<capnp::ErrorKind, Error> {
    exception
        .get_reason()?
        .to_str()
        .map_err(capnp::Error::from)?;
    Ok(match exception.get_type() {
        Ok(rpc_capnp::exception::Type::Overloaded) => capnp::ErrorKind::Overloaded,
        Ok(rpc_capnp::exception::Type::Disconnected) => capnp::ErrorKind::Disconnected,
        Ok(rpc_capnp::exception::Type::Unimplemented) => capnp::ErrorKind::Unimplemented,
        _ => capnp::ErrorKind::Failed,
    })
}
pub(crate) enum Unimplemented {
    Resolve(Option<ExportId>),
    Required,
}
/// A peer may ignore Resolve, releasing only the grant in that echoed Resolve.
/// Rejecting a required RPC message terminates the connection, as in C++.
pub(crate) fn unimplemented(
    message: rpc_capnp::message::Reader<'_>,
) -> Result<Unimplemented, Error> {
    let rpc_capnp::message::Resolve(resolve) = message.which().map_err(|_| Error::WrongMessage)?
    else {
        return Ok(Unimplemented::Required);
    };
    let id = match resolve?.which().map_err(|_| Error::WrongMessage)? {
        rpc_capnp::resolve::Exception(exception) => {
            abort_reason(exception?)?;
            None
        }
        rpc_capnp::resolve::Cap(cap) => match cap?.which().map_err(|_| Error::WrongMessage)? {
            rpc_capnp::cap_descriptor::SenderHosted(id)
            | rpc_capnp::cap_descriptor::SenderPromise(id) => Some(id),
            rpc_capnp::cap_descriptor::ThirdPartyHosted(third) => Some(third?.get_vine_id()),
            rpc_capnp::cap_descriptor::None(())
            | rpc_capnp::cap_descriptor::ReceiverHosted(_)
            | rpc_capnp::cap_descriptor::ReceiverAnswer(_) => None,
        },
    };
    Ok(Unimplemented::Resolve(id.map(ExportId::from_wire)))
}

pub fn finish_one<'pool>(
    questions: &mut questions::Questions<'_, '_>,
    messages: &mut messages::Messages<'pool, '_, '_>,
    queue: &mut messages::Queue<'pool, '_, impl Default>,
    minimum: usize,
    options: capnp::message::ReaderOptions,
) -> Result<bool, Error> {
    let Some(pending) = questions.pending_finish() else {
        return Ok(false);
    };
    let slot = queue.reserve().map_err(Error::Storage)?;
    let frame = Frame::build_bounded(
        messages,
        minimum.max(control_bytes(rpc_capnp::api::Finish::SIZE)),
        options,
        |builder| calls::finish(builder, pending.id(), pending.release_result_caps()),
    )?;
    crate::admission::finish(pending, messages, frame.lease, options)
        .map_err(Error::Output)?
        .accept(slot);
    Ok(true)
}

/// Inspect one caller-selected import slot per invocation. Returning its owner
/// transfers cleanup outside all ledger borrows; output failure keeps it live.
pub fn release_one<'pool, I>(
    index: usize,
    imports: &mut imports::Imports<'_, '_, I>,
    messages: &mut messages::Messages<'pool, '_, '_>,
    queue: &mut messages::Queue<'pool, '_, impl Default>,
    minimum: usize,
    options: capnp::message::ReaderOptions,
) -> Result<Option<I>, Error> {
    let Some(pending) = imports.pending_release(index) else {
        return Ok(None);
    };
    let slot = queue.reserve().map_err(Error::Storage)?;
    let frame = Frame::build_bounded(
        messages,
        minimum.max(control_bytes(rpc_capnp::api::Release::SIZE)),
        options,
        |builder| calls::release(builder, pending.id(), pending.count()),
    )?;
    Ok(Some(
        crate::admission::release(pending, messages, frame.lease, options)
            .map_err(Error::Output)?
            .accept(slot),
    ))
}

fn control_bytes(body: capnp::private::layout::StructSize) -> usize {
    let root = rpc_capnp::api::Message::SIZE;
    // One segment table, root pointer, envelope and fixed-size control body.
    (2 + usize::from(root.data)
        + usize::from(root.pointers)
        + usize::from(body.data)
        + usize::from(body.pointers))
        * 8
}

/// Bootstrap contains no capability parameters. Admission does not execute the
/// application, and duplicate IDs or unsupported object IDs preserve the owner.
pub fn bootstrap<'pool, 'a, A>(
    frame: Frame<'pool>,
    messages: &mut messages::Messages<'pool, '_, '_>,
    answers: &mut answers::Answers<'a, '_, A>,
    owner: A,
) -> Result<answers::Key<'a>, Rejected<'pool, A>> {
    let id = frame
        .with(messages, |root| {
            let rpc_capnp::message::Bootstrap(bootstrap) =
                root.which().map_err(|_| Error::WrongMessage)?
            else {
                return Err(Error::WrongMessage);
            };
            let bootstrap = bootstrap?;
            inbound::caller_question(bootstrap.get_question_id()).map_err(Error::Admission)?;
            if !bootstrap.get_deprecated_object_id().is_null() {
                return Err(Error::Admission(inbound::Error::Unsupported));
            }
            Ok(AnswerId::from_wire(bootstrap.get_question_id()))
        })
        .and_then(core::convert::identity);
    let id = match id {
        Ok(id) => id,
        Err(error) => {
            return Err(Rejected {
                frame,
                owner,
                error,
            })
        }
    };
    match answers.admit(id, owner) {
        Ok(key) => {
            frame.release(messages);
            Ok(key)
        }
        Err(rejected) => Err(Rejected {
            frame,
            owner: rejected.value,
            error: Error::Admission(inbound::Error::Answer(rejected.error)),
        }),
    }
}
