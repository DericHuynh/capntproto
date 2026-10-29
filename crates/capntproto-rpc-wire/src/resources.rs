//! Resource-specific retry demands. Evaluate after transactional rollback.
use capntproto_rpc_engine::{clients, messages, waiting::Requirement};
pub(crate) const NODES: usize = 0;
pub(crate) const LISTENERS: usize = 1;
pub(crate) const SUBSCRIPTIONS: usize = 2;
pub(crate) const EXPORTS: usize = 3;
pub(crate) const BUFFERS: usize = 4;
pub(crate) const OUTPUT: usize = 5;
pub(crate) const SENDS: usize = 6;
pub(crate) const OUTGOING: usize = 7;
pub(crate) const PIPELINES: usize = 8;
pub(crate) const BARRIERS: usize = 9;
pub(crate) const TASKS: usize = 10;
pub(crate) const COUNT: usize = 11;
pub(crate) fn graph_wait<'id>(
    error: clients::Error,
    graph: &impl clients::Capabilities<'id>,
) -> Option<Requirement> {
    let available = graph.resources();
    let (resource, current, limit) = match error {
        clients::Error::Full => (NODES, available.nodes, available.node_limit),
        clients::Error::ListenersFull => (LISTENERS, available.listeners, available.listener_limit),
        clients::Error::SubscriptionsFull => (
            SUBSCRIPTIONS,
            available.subscriptions,
            available.subscription_limit,
        ),
        _ => return None,
    };
    (current < limit).then_some(Requirement {
        resource,
        minimum: current.saturating_add(1),
    })
}
pub(crate) fn graph_ready<'id>(r: Requirement, graph: &impl clients::Capabilities<'id>) -> bool {
    let available = graph.resources();
    r.minimum
        <= match r.resource {
            NODES => available.nodes,
            LISTENERS => available.listeners,
            SUBSCRIPTIONS => available.subscriptions,
            _ => return false,
        }
}
pub(crate) fn message_wait(
    error: messages::Error,
    messages: &messages::Messages<'_, '_, '_>,
    queue: &messages::Queue<'_, '_, impl Sized>,
) -> Option<Requirement> {
    match error {
        messages::Error::NoBuffer => {
            let minimum = messages.failed_minimum()?;
            (messages.can_ever_reserve(minimum) && !messages.can_reserve(minimum)).then_some(
                Requirement {
                    resource: BUFFERS,
                    minimum,
                },
            )
        }
        messages::Error::NoSendSlot if queue.maximum() != 0 && queue.available() == 0 => {
            Some(Requirement {
                resource: OUTPUT,
                minimum: 1,
            })
        }
        messages::Error::NoSendSlot
            if messages.maximum_sends() != 0
                && messages.available_sends() == 0
                && queue.available() != 0 =>
        {
            Some(Requirement {
                resource: SENDS,
                minimum: 1,
            })
        }
        _ => None,
    }
}
pub(crate) fn message_ready(
    r: Requirement,
    messages: &messages::Messages<'_, '_, '_>,
    queue: &messages::Queue<'_, '_, impl Sized>,
) -> bool {
    match r.resource {
        BUFFERS => messages.can_reserve(r.minimum),
        OUTPUT => queue.available() >= r.minimum,
        SENDS => messages.available_sends() >= r.minimum,
        _ => false,
    }
}
