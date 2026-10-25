# Skip liveness lookup for ordinary aliases

The revocation conflict scan used to evaluate is_live before checking whether
either view was revocable. Checking `(revocable || entry.revocable)` first skips
the weak upgrade and liveness call when both flags are false. The liveness method
is private, only reads a Cell, and has no user callback. Revocable conflict and
expired-owner behavior remain the same.

The actual no_std fixture's nine tests and Clippy pass, including the precise
revocable-alias rejection reason. Five warmed 32-client allocation samples remain
128 blocks / 14,720 bytes with zero spread, as expected for a read-work change.
No new tests duplicate the Boolean expression: existing behavioral tests exercise
identity reuse, revocation, cancellation, callback and deep/concurrent dispatch.
Compiled work and simulated branches require separate qualification against the
fused-state source; no native latency claim is made.
