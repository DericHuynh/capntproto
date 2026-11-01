//! A scatter/gather-preserving Tokio adapter for the legacy RPC byte streams.
mod futures_io;
pub use futures_io::FuturesIo;
