# RPC state and message utilities

Shared utilities used by the legacy RPC driver and framing layer: identifiers,
reference credits, completions, ready queues, tables, message leases and IO
queues. This crate is not an independent RPC implementation.

The crate is `no_std`; fixed-storage operations do not allocate. The optional
`alloc` feature enables growing storage and owned completions. The hosted legacy
RPC system uses that feature and is not a no-alloc driver.
