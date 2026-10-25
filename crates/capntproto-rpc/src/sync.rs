//! Hosted mutex remains canonical; alloc-only wake mailboxes use the futures spin backend.
#[cfg(feature = "std")]
pub(crate) use std::sync::Mutex;
#[cfg(not(feature = "std"))]
#[derive(Default)]
pub(crate) struct Mutex<T>(spin::Mutex<T>);
#[cfg(not(feature = "std"))]
impl<T> Mutex<T> {
    pub(crate) fn lock(&self) -> Result<spin::MutexGuard<'_, T>, core::convert::Infallible> {
        Ok(self.0.lock())
    }
}
