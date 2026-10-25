# Controlled comparison of the superseded forced-counter selector

[Immutable run40209023844](https://github.com/DericHuynh/capntproto/actions/runs/40209023844) compares original source c6900f3 to14e9264. ReturnOwnership::release is byte-for-byte source-identical on both sides. Only scheduler source changes: the single-byte Publication and forced conditional counter selection. Scoped engine/actual legacy tests and all240 profile trials pass. Full hosted checks and both-width ownership Miri were qualified in the preceding runs; this final profiling workflow does not claim a duplicate full test/Miri matrix.

All five samples show exclusive scheduler Ir increases39 for unary/pipelined,78 dependent,95 callback,44 concurrent/deep (paired SD0), Bc falls2..6, and Bi is unchanged. Whole Ir means regress0.004..0.152%. Whole Bcm improves dependent2.042% and concurrent4.834%, but regresses unary4.771%, pipelined3.046%, callback6.863% and deep38.363%. Whole Bim regresses in five cases (max19.110%); callback improves. TLS blocks stay7/57/202; byte and QUIC regressions remain in comparison.md/json.

The actual TaskSet::poll body grows1157->1163 non-padding instructions, with179->177 conditional jumps and3 conditional moves on each side.665/715 common normalized retained bodies match. This is a separate control for the combined counter representation/selector; its mixed result does not establish a native speedup. The forced-counter selector is superseded by conditional threshold selection, which shares the callback branch.

Five fresh-process repetitions retain means, sample SD and every regression. Simulated miss events and startup/crypto slope noise are not hardware latency. Complete no-alloc legacy connections remain unimplemented.
