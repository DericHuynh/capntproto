//! Question reference storage without prescribed context/producer/pipeline owners.
use crate::completion::direct::Reply;

/// An immutable projection of existing fields. It preserves a hosted record's
/// physical layout and destructor instead of nesting another owning record.
/// The projection itself owns no protocol interest or storage.
pub struct View<'a, C, K, T, P> {
    pub context: &'a Option<C>,
    pub id: &'a K,
    pub reply: &'a Reply<T>,
    pub pipeline: &'a Option<P>,
}
impl<'a, C, K, T, P> View<'a, C, K, T, P> {
    #[inline(always)]
    pub fn context(self) -> Option<&'a C> {
        self.context.as_ref()
    }
    #[inline(always)]
    pub fn id(self) -> &'a K {
        self.id
    }
    #[inline(always)]
    pub fn reply(self) -> &'a Reply<T> {
        self.reply
    }
    #[inline(always)]
    pub fn pipeline(self) -> Option<&'a P> {
        self.pipeline.as_ref()
    }
}
/// Exclusive projections return detached owners to the caller, so destruction
/// can occur after releasing any table/record borrow. No finalization authority
/// or storage lifetime is added by constructing these parts.
pub struct Parts<'a, C, F, T, P> {
    pub context: &'a mut Option<C>,
    pub producer: &'a mut Option<F>,
    pub reply: &'a mut Reply<T>,
    pub pipeline: &'a mut Option<P>,
}
impl<'a, C, F, T, P> Parts<'a, C, F, T, P> {
    #[inline(always)]
    pub fn take_context(self) -> Option<C> {
        self.context.take()
    }
    #[inline(always)]
    pub fn take_producer(self) -> Option<F> {
        self.producer.take()
    }
    #[inline(always)]
    pub fn reply_mut(self) -> &'a mut Reply<T> {
        self.reply
    }
    #[inline(always)]
    pub fn replace_pipeline(self, pipeline: P) -> Option<P> {
        self.pipeline.replace(pipeline)
    }
}

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
    #[inline(always)]
    pub fn view(&self) -> View<'_, C, K, T, P> {
        View {
            context: &self.context,
            id: &self.id,
            reply: &self.reply,
            pipeline: &self.pipeline,
        }
    }
    #[inline(always)]
    pub fn parts(&mut self) -> Parts<'_, C, F, T, P> {
        Parts {
            context: &mut self.context,
            producer: &mut self.producer,
            reply: &mut self.reply,
            pipeline: &mut self.pipeline,
        }
    }

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
        self.view().context()
    }
    #[inline]
    pub fn take_context(&mut self) -> Option<C> {
        self.parts().take_context()
    }
    #[inline]
    pub fn id(&self) -> &K {
        self.view().id()
    }
    #[inline]
    pub fn take_producer(&mut self) -> Option<F> {
        self.parts().take_producer()
    }
    #[inline]
    pub fn reply(&self) -> &Reply<T> {
        self.view().reply()
    }
    #[inline]
    pub fn reply_mut(&mut self) -> &mut Reply<T> {
        self.parts().reply_mut()
    }
    #[inline]
    pub fn pipeline(&self) -> Option<&P> {
        self.view().pipeline()
    }
    /// Return a replaced owner for destruction after releasing the record borrow.
    #[inline]
    pub fn replace_pipeline(&mut self, pipeline: P) -> Option<P> {
        self.parts().replace_pipeline(pipeline)
    }
}

#[cfg(test)]
mod tests;
