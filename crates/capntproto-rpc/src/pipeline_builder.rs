//! Owned construction of result pipelines independent of response allocation.
use capnp::{
    any_pointer,
    capability::FromTypelessPipeline,
    message,
    private::layout::CapTable,
    traits::{ImbueMut, Owned, Pipelined},
};
use core::marker::PhantomData;

/// Build a typed capability pipeline independently of a server's response.
/// Fill capabilities through `get()`, then consume the builder with `build()`.
/// Scalar data is private construction storage; callers only see capabilities.
/// The final response must contain the same capabilities (or promises resolving
/// to them). This identity contract is the server's responsibility, as in C++.
pub struct PipelineBuilder<T: Owned> {
    message: message::Builder<message::HeapAllocator>,
    caps: CapTable,
    marker: PhantomData<T>,
}
impl<T: Owned> Default for PipelineBuilder<T> {
    fn default() -> Self {
        Self::new()
    }
}
impl<T: Owned> PipelineBuilder<T> {
    /// Use a 64-word first segment, matching the C++ pipeline builder default.
    pub fn new() -> Self {
        Self::with_first_segment_words(64)
    }

    /// Set an allocation hint. The arena grows if the pipeline needs more space.
    pub fn with_first_segment_words(words: u32) -> Self {
        let mut message =
            message::Builder::new(message::HeapAllocator::new().first_segment_words(words));
        message.init_root::<T::Builder<'_>>();
        Self {
            message,
            caps: CapTable::default(),
            marker: PhantomData,
        }
    }
    pub fn get(&mut self) -> T::Builder<'_> {
        let mut root: any_pointer::Builder = self.message.get_root().expect("owned pipeline root");
        root.imbue_mut(&mut self.caps);
        root.get_as().expect("initialized pipeline struct")
    }
    /// Freeze the message and transfer capability ownership to the pipeline.
    pub fn build(self) -> T::Pipeline
    where
        T: Pipelined,
        T::Pipeline: FromTypelessPipeline,
    {
        FromTypelessPipeline::new(any_pointer::Pipeline::new(Box::new(
            crate::local::Pipeline::new(Box::new(crate::local::ResultsDone::new(
                self.message,
                self.caps,
            ))),
        )))
    }
}
