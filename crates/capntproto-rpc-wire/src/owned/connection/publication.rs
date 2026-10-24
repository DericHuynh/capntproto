//! Proof identity shared by immutable hosted Call and Return publication.
//! A successful walk proves pointer/capability bounds for exactly these limits.
//! Descriptor authority and resource ownership are still checked at publication.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Validation {
    capabilities: u32,
    traversal: Option<usize>,
    nesting: core::num::NonZeroI32,
}
impl Validation {
    pub(super) fn new(capabilities: u32, options: capnp::message::ReaderOptions) -> Option<Self> {
        // Zero cannot validate the immutable RPC ancestors. Its niche keeps
        // an absent certificate inline without a separate tag in every owner.
        Some(Self {
            capabilities,
            traversal: options.traversal_limit_in_words,
            nesting: core::num::NonZeroI32::new(options.nesting_limit)?,
        })
    }
}
