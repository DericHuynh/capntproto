use super::*;

/// Explicit hosted storage policy. Bounds apply independently to each resource,
/// not to the aggregate size of application-owned requests or service futures.
/// Growing tables start empty. Exhaustion fails rather than falling back to the
/// legacy driver or an unbounded allocation provider.
#[derive(Clone, Copy)]
pub struct Options {
    pub connections: NonZeroUsize,
    /// Uniform per-table bound, including graph clients and protocol ledgers.
    pub table_entries: NonZeroUsize,
    pub adopted_answers: usize,
    /// Includes the target receipt of an incoming Call.
    pub message_capabilities: usize,
    pub pipeline_path: usize,
    pub hooks: NonZeroUsize,
    pub concurrency: NonZeroUsize,
    /// Shared transport frames and data-only application payloads pin their
    /// allocated buffers until their last owner drops, including after return.
    pub message_buffers: NonZeroUsize,
    pub message_bytes: NonZeroUsize,
    /// Includes buffer alignment padding and retained allocator capacity.
    pub retained_bytes: NonZeroUsize,
    /// Separate bound on released application builder segments cached for
    /// reuse. Live requests/results are not part of this cache. Zero disables it.
    pub cached_message_words: usize,
    pub cached_message_segments: usize,
    pub output_queue: NonZeroUsize,
    /// Cooperative work/read/write allowance per poll. The hosted default of
    /// 128 lets the ordinary call and cleanup lanes complete in one turn while
    /// retaining the coordinator's separate 32-frame input bound.
    pub poll_budget: NonZeroUsize,
    pub reader: capnp::message::ReaderOptions,
    pub maximum_fds: usize,
}
impl Options {
    /// Apply the supplied entry bound to each table, hook/call queue and message
    /// slot independently. Tune public fields before construction for unequal
    /// budgets. No byte buffers are preallocated. At least two entries and room
    /// for a framed message plus alignment padding are required.
    pub fn bounded(
        connections: NonZeroUsize,
        entries: NonZeroUsize,
        message_bytes: NonZeroUsize,
        retained_bytes: NonZeroUsize,
    ) -> capnp::Result<Self> {
        let options = Self {
            connections,
            table_entries: entries,
            adopted_answers: entries.get(),
            message_capabilities: entries.get(),
            pipeline_path: entries.get(),
            hooks: entries,
            concurrency: entries,
            message_buffers: entries,
            message_bytes,
            retained_bytes,
            cached_message_words: (retained_bytes.get() / 8).min(16 * 1024),
            // Match the two-party builder cache's segment bound. A separate
            // word limit still caps retained bytes; the former 16-segment cap
            // discarded small buffers while most of that byte budget was free.
            cached_message_segments: entries.get().min(64),
            output_queue: entries,
            poll_budget: NonZeroUsize::new(128).unwrap(),
            reader: Default::default(),
            maximum_fds: 253,
        };
        options.validate()?;
        Ok(options)
    }
    pub(super) fn validate(&self) -> capnp::Result<()> {
        if self.table_entries.get() < 2
            || self.message_buffers.get() < 2
            || self.message_bytes.get() < 128
            || self
                .message_bytes
                .get()
                .checked_add(7)
                .is_none_or(|minimum| minimum > self.retained_bytes.get())
        {
            return Err(Error::failed(
                "portable RPC needs two table/buffer slots, a 128-byte frame, and its padded storage".into()
            ));
        }
        if self.message_capabilities > u32::MAX as usize {
            return Err(Error::failed(
                "capability count exceeds the wire format".into(),
            ));
        }
        Ok(())
    }
}
