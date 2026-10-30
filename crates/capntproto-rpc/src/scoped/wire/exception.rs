//! One exception encoding for hosted Returns and terminal Aborts.
use capnp::{field_api::FixedStruct, Error, ErrorKind};
use capntproto_rpc_wire::rpc_capnp::api::{
    exception::{Detail, Type},
    Exception,
};
use std::borrow::Cow;

pub(super) fn reason(error: &Error) -> Cow<'_, str> {
    match error.kind {
        ErrorKind::Failed
        | ErrorKind::Disconnected
        | ErrorKind::Overloaded
        | ErrorKind::Unimplemented => Cow::Borrowed(&error.extra),
        _ => Cow::Owned(error.to_string()),
    }
}

pub(super) fn write(
    error: &Error,
    trace: Option<&str>,
    mut output: FixedStruct<'_, Exception>,
) -> capnp::Result<()> {
    output.scalar(Exception::TYPE).set(match error.kind {
        ErrorKind::Disconnected => Type::Disconnected,
        ErrorKind::Overloaded => Type::Overloaded,
        ErrorKind::Unimplemented | ErrorKind::SettingDynamicCapabilitiesIsUnsupported => {
            Type::Unimplemented
        }
        _ => Type::Failed,
    });
    output
        .field(Exception::REASON)
        .copy_from(reason(error).as_ref())?;
    if let Some(trace) = trace {
        output.field(Exception::TRACE).copy_from(trace)?;
    }
    let count = error.details().count();
    if count != 0 {
        let mut details = output.field(Exception::DETAILS).init(count)?;
        for (index, (id, data)) in error.details().enumerate() {
            let mut detail = details.get_mut(index).expect("allocated details");
            detail.scalar(Detail::DETAIL_ID).set(id);
            detail.field(Detail::DATA).copy_from(data)?;
        }
    }
    Ok(())
}

pub(super) fn publication_bytes(error: &Error, trace: Option<&str>) -> capnp::Result<usize> {
    let overflow = || Error::from_kind(ErrorKind::MessageSizeOverflow);
    let mut size = 128usize
        .checked_add(reason(error).len())
        .ok_or_else(overflow)?;
    if let Some(trace) = trace {
        size = size
            .checked_add(trace.len())
            .and_then(|n| n.checked_add(8))
            .ok_or_else(overflow)?;
    }
    for (_, data) in error.details() {
        size = size
            .checked_add(24)
            .and_then(|n| n.checked_add(data.len()))
            .ok_or_else(overflow)?;
    }
    Ok(size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use capntproto_rpc_wire::rpc_capnp::api::Message;
    #[test]
    fn terminal_exception_encoding_keeps_owned_diagnostics_without_allocating() {
        let mut error = Error::overloaded("owned reason".into());
        error.set_remote_trace("remote trace must not be forwarded".into());
        error.set_detail(11, b"opaque\0bytes".to_vec());
        error.set_detail(12, Vec::new());
        let mut words = [capnp::word(0, 0, 0, 0, 0, 0, 0, 0); 64];
        let measured = allocation_counter::measure(|| {
            let mut builder = capnp::message::FixedBuilder::new(&mut words).unwrap();
            let output = builder
                .try_init_root::<Message>()
                .unwrap()
                .into_field(Message::ABORT)
                .init()
                .unwrap();
            write(&error, Some("local trace"), output).unwrap();
            assert!(
                builder.size_in_words() * 8 + 8
                    <= publication_bytes(&error, Some("local trace")).unwrap()
            );
            let root = builder
                .get_root_as_reader::<capntproto_rpc_wire::rpc_capnp::message::Reader>()
                .unwrap();
            let capntproto_rpc_wire::rpc_capnp::message::Abort(exception) = root.which().unwrap()
            else {
                panic!("Abort")
            };
            let exception = exception.unwrap();
            assert_eq!(
                exception.get_reason().unwrap().to_str().unwrap(),
                "owned reason"
            );
            assert_eq!(
                exception.get_trace().unwrap().to_str().unwrap(),
                "local trace"
            );
            assert_eq!(
                exception.get_details().unwrap().get(0).get_data().unwrap(),
                b"opaque\0bytes"
            );
            assert_eq!(
                exception.get_details().unwrap().get(1).get_data().unwrap(),
                b""
            );
        });
        assert_eq!(measured.count_total, 0);
    }
}
