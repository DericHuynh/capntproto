//! Question reference storage without prescribed context/producer/pipeline owners.
use crate::completion::direct::Reply;

/// Internal record data. C can retain a hosted context or identify a connection
/// by key; P can retain a hosted weak owner or identify a pipeline by key. T may
/// borrow independently retained frame/service storage and need not be static.
///
/// The driver owns last-interest cleanup and validates all keys. Caller-backed
/// records must not store public leases borrowing their own containing context;
/// those finalizers live outside it. Merely dropping this storage does not grant
/// protocol authority to emit Finish or recycle its wire ID.
pub struct Reference<C, K, F, T, P> {
    context: Option<C>,
    id: K,
    producer: Option<F>,
    reply: Reply<T>,
    pipeline: Option<P>,
}
impl<C, K, F, T, P> Reference<C, K, F, T, P> {
    pub fn new(context: Option<C>, id: K, producer: Option<F>) -> Self {
        Self {
            context,
            id,
            producer,
            reply: Reply::new(),
            pipeline: None,
        }
    }
    #[inline]
    pub fn context(&self) -> Option<&C> {
        self.context.as_ref()
    }
    #[inline]
    pub fn take_context(&mut self) -> Option<C> {
        self.context.take()
    }
    #[inline]
    pub fn id(&self) -> &K {
        &self.id
    }
    #[inline]
    pub fn take_producer(&mut self) -> Option<F> {
        self.producer.take()
    }
    #[inline]
    pub fn reply(&self) -> &Reply<T> {
        &self.reply
    }
    #[inline]
    pub fn reply_mut(&mut self) -> &mut Reply<T> {
        &mut self.reply
    }
    #[inline]
    pub fn pipeline(&self) -> Option<&P> {
        self.pipeline.as_ref()
    }
    /// Return a replaced owner for destruction after releasing the record borrow.
    #[inline]
    pub fn replace_pipeline(&mut self, pipeline: P) -> Option<P> {
        self.pipeline.replace(pipeline)
    }
}

#[cfg(test)]
mod tests;
