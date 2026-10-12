use super::*;
use capntproto_rpc_engine::{
    flow_control::Window,
    ids::{ImportId, QuestionId, WireId},
};
use capntproto_rpc_wire::{calls, pipeline::Path, rpc_capnp};
use portable_capnp::{
    api::{
        echo::{NestedResults, WriteParams},
        pipeline_node::Group,
        PipelineNode,
    },
    echo,
};

pub fn bounded_pipeline_and_streaming() -> Result<()> {
    let mut path_storage = [99; 4];
    let path = echo::Owned::NESTED
        .pipeline(&mut path_storage)
        .field(NestedResults::VALUE)
        .unwrap()
        .field(PipelineNode::CHILD)
        .unwrap()
        .group(PipelineNode::GROUP)
        .unwrap()
        .capability(Group::ECHO)
        .unwrap();
    assert_eq!(path.fields(), &[0, 1, 2]);
    let mut words = [ZERO; 80];
    let mut output = message::FixedBuilder::new(&mut words)?;
    {
        let mut root = output.try_init_root::<NestedResults>()?;
        let mut node = root.field(NestedResults::VALUE).init()?;
        let mut child = node.field(PipelineNode::CHILD).init()?;
        child
            .group(PipelineNode::GROUP)
            .edit()?
            .field(Group::ECHO)
            .set_index(3)?;
    }
    let content = output.get_root_as_reader::<capnp::any_pointer::Reader<'_>>()?;
    assert_eq!(Path::new(path.fields()).resolve(content)?, 3);
    assert_eq!(path_storage, [0, 1, 2, 99]);

    let mut small = [99];
    let rejected = echo::Owned::NESTED
        .pipeline(&mut small)
        .field(NestedResults::VALUE)
        .unwrap()
        .capability(PipelineNode::ECHO)
        .unwrap_err();
    assert_eq!(
        rejected.error,
        capnp::wire_capability::path::Error::Capacity
    );
    assert_eq!(small, [0]);
    let mut union_storage = [99; 2];
    let rejected = echo::Owned::NESTED
        .pipeline(&mut union_storage)
        .field(NestedResults::VALUE)
        .unwrap()
        .capability(PipelineNode::SELECTED)
        .unwrap_err();
    assert_eq!(
        rejected.error,
        capnp::wire_capability::path::Error::UnionSelection
    );
    assert_eq!(union_storage, [0, 99]);

    // Union capability readers keep their message lifetime, even in generated
    // legacy Which aliases. Their fixed-buffer writer remains allocation-free.
    output.reset();
    output
        .try_init_root::<PipelineNode>()?
        .field(PipelineNode::SELECTED)
        .set_index(0)?;
    let node = output.get_root_as_reader::<portable_capnp::pipeline_node::Reader<'_>>()?;
    let portable_capnp::pipeline_node::Selected(cap) = node.which().unwrap() else {
        panic!("union arm");
    };
    assert_eq!(cap?.index(), 0);

    output.reset();
    calls::streaming_call(
        &mut output,
        QuestionId::from_wire(7),
        calls::Target::Imported(ImportId::from_wire(9)),
        echo::Owned::WRITE,
        |mut params| {
            params.scalar(WriteParams::VALUE).set(42);
            Ok(())
        },
    )?;
    let mut credit = Window::fixed(128);
    let now = core::time::Duration::from_secs(1);
    let sent = credit
        .sent((output.size_in_words() * 8) as u128, now)
        .unwrap();
    let message = output.get_root_as_reader::<rpc_capnp::message::Reader<'_>>()?;
    let rpc_capnp::message::Call(call) = message.which().unwrap() else {
        panic!("Call");
    };
    let call = call?;
    assert!(call.get_no_promise_pipelining());
    let incoming = calls::Incoming::new(call);
    assert!(incoming.dispatch(echo::Owned::ECHO).is_err());
    let dispatch = incoming.dispatch_streaming(echo::Owned::WRITE)?;
    assert_eq!(dispatch.params.value(), 42);
    assert!(!dispatch.allow_cancellation());
    let mut reply_words = [ZERO; 24];
    let mut reply = message::FixedBuilder::new(&mut reply_words)?;
    dispatch.acknowledge(&mut reply)?;
    let response = reply.get_root_as_reader::<rpc_capnp::message::Reader<'_>>()?;
    let rpc_capnp::message::Return(result) = response.which().unwrap() else {
        panic!("Return");
    };
    let result = result?;
    assert_eq!(result.get_answer_id(), 7);
    let rpc_capnp::return_::Results(payload) = result.which().unwrap() else {
        panic!("Results");
    };
    let payload = payload?;
    assert!(payload.get_content().is_null() && payload.get_cap_table()?.is_empty());
    credit
        .ack(sent, now + core::time::Duration::from_millis(10))
        .unwrap();
    assert_eq!(credit.in_flight(), 0);
    Ok(())
}
