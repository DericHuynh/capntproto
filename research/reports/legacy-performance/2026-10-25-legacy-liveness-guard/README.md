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


## Qualification

Source `3763d492c61e24a7f3163e71806b95ec7ffe050f`, control
`a80ba8b7e26982414c0157207f8f81c51f1fede6`, qualification
`7ef757fe4dacea3ebb4bd7616fcc67e99050979e`,
[run 40176721991](https://github.com/DericHuynh/capntproto/actions/runs/40176721991)
passes full hosted verification and all 240 repeated trials. No new Miri run is
claimed for this safe Boolean guard reorder; the preceding ownership change has
separate two-width Miri evidence. CI allocation samples again match 128/14,720.

Local plus scheduler Ir/Bc/Bi slopes remain exact in all six cases; the alias scan
is setup work, so zero slope does not prove zero setup savings. 325/325 normalized
common retained assembly bodies match. TLS block slopes remain 7/57/202. All byte
and QUIC samples/spreads remain in comparison.json. Whole Ir ranges -0.063% to
+0.042%. Simulated Bim improves 8.157% concurrent and 2.983% deep but regresses
1.040% dependent, 0.317% pipeline and 2.475% callback. Bcm regresses in five cases,
including +8.489% concurrent. These are measured regressions, not native timing.

Verified artifact 12593039838: 81,683,732 bytes, SHA-256
`f207828d997046a6065ec2c05f2d146bcd08ad6256b472ae17071964a5e78665`.
Full raw assembly/symbols/profiles remain in that archive; selected attribution
reconciles every whole-program delta. Reproduce with analyze.py.
