//! Protocol obligations for imported promises, independent of IO and scheduling.
//!
//! A descriptor increments references elsewhere. Resolve itself never does.
//! Keeping this state until the last wire reference is released distinguishes
//! duplicate resolution from a legitimate late Resolve for an absent import.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImportState {
    #[default]
    Hosted,
    Pending,
    Resolved,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    AlreadyResolved,
    NotPromise,
}
impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::AlreadyResolved => "promise ID was already resolved and has not been released",
            Self::NotPromise => "Resolve refers to a non-promise import",
        })
    }
}
impl core::error::Error for Error {}
impl ImportState {
    /// Preview a received descriptor without committing a reference or state.
    /// A senderHosted alias preserves an outstanding promise obligation.
    pub fn descriptor(self, promise: bool) -> Result<Self, Error> {
        if !promise {
            return Ok(self);
        }
        match self {
            Self::Hosted | Self::Pending => Ok(Self::Pending),
            Self::Resolved => Err(Error::AlreadyResolved),
        }
    }
    pub fn resolution(self) -> Result<Self, Error> {
        match self {
            Self::Pending => Ok(Self::Resolved),
            Self::Hosted => Err(Error::NotPromise),
            Self::Resolved => Err(Error::AlreadyResolved),
        }
    }
}
