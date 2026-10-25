//! Invocation ownership is separate from admission and cancellation policy.
use super::super::{Params, PayloadStorage, Results};
use crate::{private::capability::ParamsHook, traits::Owned};

/// A single invocation's storage and ownership transfer. Taking consumes the
/// original provider; it cannot create a second producer or completion owner.
/// Implementations choose typed views and their completion representation.
pub trait CallStorage<P, R>: Sized {
    type Params<'a>
    where
        Self: 'a;
    type Results<'a>
    where
        Self: 'a;
    type Taken;
    type Completion;
    fn get(&mut self) -> crate::Result<(Self::Params<'_>, Self::Results<'_>)>;
    fn take(self) -> Self::Taken;
    fn complete(self, result: crate::Result<()>) -> Self::Completion;
}

/// Concrete parameter/result owners for one admitted call. The driver observes
/// the returned Result to decide its Return outcome; this owner does not send a
/// Return or change cancellation policy. Parameters drop before the result sink.
pub struct PreparedCall<PH, RH> {
    params: PH,
    results: RH,
}
impl<PH, RH> PreparedCall<PH, RH> {
    pub fn new(params: PH, results: RH) -> Self {
        Self { params, results }
    }
    /// Transfer both original owners once, including their borrows and guards.
    pub fn into_owners(self) -> (PH, RH) {
        (self.params, self.results)
    }
}
impl<P: Owned, R, PH: ParamsHook, RH: PayloadStorage<R>> CallStorage<P, R>
    for PreparedCall<PH, RH>
{
    type Params<'a>
        = P::Reader<'a>
    where
        Self: 'a;
    type Results<'a>
        = RH::Builder<'a>
    where
        Self: 'a;
    type Taken = (Params<P, PH>, Results<R, RH>);
    type Completion = crate::Result<()>;
    fn get(&mut self) -> crate::Result<(Self::Params<'_>, Self::Results<'_>)> {
        Ok((self.params.get()?.get_as()?, self.results.try_get(None)?))
    }
    fn take(self) -> Self::Taken {
        (
            Params::from_owner(self.params),
            Results::from_owner(self.results),
        )
    }
    fn complete(self, result: crate::Result<()>) -> Self::Completion {
        drop(self);
        result
    }
}

#[cfg(feature = "alloc")]
impl<P: Owned, R: Owned> CallStorage<P, R>
    for &mut dyn crate::private::capability::CallContextHook
{
    type Params<'a>
        = P::Reader<'a>
    where
        Self: 'a;
    type Results<'a>
        = R::Builder<'a>
    where
        Self: 'a;
    type Taken = (Params<P>, Results<R>);
    type Completion = super::super::Promise<(), crate::Error>;
    #[inline(always)]
    fn get(&mut self) -> crate::Result<(Self::Params<'_>, Self::Results<'_>)> {
        let (params, results) = self.borrow();
        Ok((params.get()?.get_as()?, results.get()?.get_as()?))
    }
    #[inline(always)]
    fn take(self) -> Self::Taken {
        let (params, results) = crate::private::capability::CallContextHook::take(self);
        (Params::new(params), Results::new(results))
    }
    #[inline(always)]
    fn complete(self, result: crate::Result<()>) -> Self::Completion {
        super::super::Promise::from(result)
    }
}
