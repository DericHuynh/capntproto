# Direct scheduler follow-up

Source `0c44814`, baseline `35b0aad`; qualification `46fc951926a158f2dfe31d9849c71718e39bc8f5`, [run 40165359321](https://github.com/DericHuynh/capntproto/actions/runs/40165359321), succeeded. Artifact 12588389122 is 81,201,893 bytes with verified SHA-256 `e3b2a58418608ae7a55a4c8b668231a69cdccd824e6d428e326f216fdbc609e9`.

The source-closed check is fused into one hosted source selection, the regular policy is forced into its caller, and the private hosted reaper vtable is retained behind a static engine provider. All 890 unit/integration plus two support doc tests passed, as did target/Clippy/format checks and the scheduler probe on the host and M4 link. No unsafe storage/waker code changed, so the prior two-width Miri qualification is not repeated here.

Combined scheduler self Ir falls by exactly 31/62/31/42/83/42 per operation in unary/dependent/pipeline/concurrent/callback/deep, zero paired SD. Component Bc and Bi are unchanged in every repetition. This improves instructions but does not remove the conditional and callback-indirect additions introduced by scheduler extraction. The hosted adapter still translated between two equivalent command enums; a separate follow-up removes that conversion and is not included here.

TLS block slopes remain 7/58/202. All byte and nonzero QUIC differences/spreads remain in comparison.json. Simulated concurrency Bim rises 21.138%, deep rises 4.897%; conditional misses mostly improve. Native latency and hardware branch prediction remain unqualified. No local timings are used, and this is not complete legacy no-allocator RPC.
