# C++ selected-payload readers on Tokio

Prepared requests and returns now read the payload through the pointer already selected by their owning builder. They avoid re-decoding immutable Call/Return envelopes on each read. Whole-process instructions fall **1.04% on the pipeline server** and **0.46% on the callback client**. Allocation counts are unchanged. This is an instruction reduction, not a demonstrated latency improvement; the 5% gap remains open.

## Mapping to the C++ implementation

The source is the pinned Cap'n Proto checkout `0de72d8d8cec6b69edaa29de51d3bd490341f9c2`, with its upstream MIT license preserved. The port follows the direct payload access and ownership pattern in `vendor/capnproto/c++/src/capnp/rpc.c++`:

| C++ implementation | Rust implementation |
| --- | --- |
| `RpcRequest::getRoot`, line 2496, returns retained `paramsBuilder` | `PreparedCall::params_reader` reads the selected parameter pointer; local request/parameter hooks use it and imbue their capability table. |
| `RpcServerResponseImpl::getResultsBuilder`, line 3003, returns the retained payload content | `PreparedReturn::reader` directly reads its selected result pointer. |
| `RpcResponseImpl::getResults`, line 2924, and `RpcCallContext::getParams`, line 3366, return stored readers backed by their owning messages | `SelectedBuilder::get_reader` borrows the owner and reconstructs a reader from its stable segment/offset. The retained-parameter test also suspends after request/graph/pool teardown. |

Rust cannot store an unrestricted reference into a movable arena as C++ does. The existing selection prevents ancestor replacement; the new accessor resolves the current segment base and checks the pointer word before borrowing it. Descendants may grow, move to other segments, or change type between borrows. This uses the ordinary builder-reader policy. Publication still uses separate checked readers with traversal/nesting limits and capability-index validation. There is no new unsafe code in the RPC or wire crates. The core block is documented and covered by Miri.

## Rejected ownership prototype

An inline admitted-parameter owner saved one allocation per eligible synchronous call: pipeline-server malloc calls fell from 29,912 to 27,711 and unary-server calls from 2,409 to 1,308. However, server instructions increased 0.708% and 1.282%, respectively. It was reverted. Its exact source, binary, twelve profiles and measurements are retained separately; the accepted implementation keeps the boxed parameter owner. Fewer allocations alone did not justify the ownership change.

## Instruction and wire evidence

All final profiles include process startup, 100 warmups and 1,000 measured operations. Each pair uses the frozen baseline or candidate on both peers. The legacy and C++ controls use the same workflow operations. These instruction totals are whole-process diagnostics and do not predict end-to-end latency by themselves.

| Workflow | Peer | Before instructions | After instructions | Change | Legacy | C++ |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| pipelined-read | client | 72,665,608 | 72,631,870 | -0.046% | 54,688,526 | 67,540,923 |
| pipelined-read | server | 64,121,738 | 63,453,652 | -1.042% | 56,421,188 | 76,871,825 |
| unary-empty | client | 16,847,254 | 16,847,133 | -0.001% | 17,805,766 | 31,809,110 |
| unary-empty | server | 15,077,884 | 15,077,507 | -0.003% | 16,283,723 | 32,673,786 |
| callback | client | 56,139,967 | 55,880,737 | -0.462% | 36,425,126 | 67,987,362 |
| callback | server | 56,245,901 | 56,247,346 | +0.003% | 45,075,371 | 63,180,543 |

All six before/after peer comparisons have identical complete write bytes and groups. Twenty-four syscall journals cover baseline, candidate, legacy and C++, with every successful write decoded. The steady pipeline matches C++: a 1,368-byte Finish/Release/Call/Call client write and paired 1,216-byte Returns. Startup and cleanup differences are retained. No batching policy, scheduling budget, fixture, deadline or admission limit changed.

## Latency, including regressions

The initial campaign uses server CPU 1 and client CPU 2, five rotating repetitions, 100 warmups and 1,000 samples per trial for all twelve workflows: 240,000 uninstrumented samples. Portable meets 8/12 local faster-reference cells and 9/12 against C++ alone. This local TCP topology does not replace the dedicated twelve-remote/four-loopback campaign.

| Workflow | Before µs | After µs | Legacy µs | C++ µs | After / faster reference |
| --- | ---: | ---: | ---: | ---: | ---: |
| unary-empty | 20.410 | 20.330 | 19.929 | 21.882 | 1.020× |
| unary-small | 23.380 | 31.036 | 21.342 | 21.962 | 1.454× |
| unary-1k | 20.530 | 20.530 | 20.184 | 22.543 | 1.017× |
| fanout-32 | 130.067 | 117.108 | 138.043 | 147.662 | 0.848× |
| dependent-read | 58.096 | 55.002 | 67.119 | 70.404 | 0.819× |
| pipelined-read | 43.534 | 52.357 | 41.416 | 33.018 | 1.586× |
| write-read | 63.264 | 44.716 | 40.219 | 66.278 | 1.112× |
| callback | 51.281 | 70.954 | 42.773 | 67.435 | 1.659× |
| bulk-64k | 51.916 | 48.222 | 105.505 | 53.299 | 0.905× |
| bulk-256k | 133.933 | 109.651 | 157.507 | 263.513 | 0.696× |
| bulk-1m | 444.520 | 419.661 | 793.265 | 940.747 | 0.529× |
| bulk-4m | 3243.198 | 3185.182 | 4394.143 | 5037.159 | 0.725× |

Because small unary, pipeline and callback pooled latencies regressed, a separate balanced twenty-repetition campaign repeats those cases, retaining another 240,000 samples. Its results are not pooled with the first campaign.

| Workflow | Before µs | After µs | Legacy µs | C++ µs | After / faster reference |
| --- | ---: | ---: | ---: | ---: | ---: |
| pipelined-read | 45.677 | 49.854 | 29.504 | 31.266 | 1.690× |
| unary-small | 21.582 | 20.871 | 21.712 | 24.436 | 0.961× |
| callback | 54.916 | 54.926 | 44.485 | 47.455 | 1.235× |

Pipeline pooled p50 remains 9.14% higher; small unary improves 3.29%, while callback changes +0.02%. Pipeline trial medians span about 41–89 µs in both builds. Median paired trial ratios are 0.997 pipeline, 1.001 unary and 0.992 callback. Those observations show substantial run-to-run variability but do not erase the pooled regression or establish a latency win. The retained benefit is less work on measured code paths. All samples, p95/p99 and per-repetition medians are preserved in the raw archive and [report.json](report.json). Gap #11 remains open.

## Validation and reproduction

Six selected-builder tests, 307 RPC tests, 146 wire tests, three Tokio adapter tests and 192 mixed-peer workflow combinations pass. Miri passes all six selected-builder tests and the retained-parameter test that suspends after storage teardown. ARM checks pass with and without alloc. Core/wire/RPC and Tokio harness Clippy pass with warnings denied. The workspace unsafe documentation gate passes after reviewing the source-bound core fingerprint; its diagnostic allowances are unchanged. Coverage has not been requalified and its floors are unchanged.

The [source patch](source.patch) applies to `992a2b49583998edefda3e41edcef290b6d45968`, reconstructing measured tree `5d704ddbacf08fd9aa38853007a2e0762f2567c4`. The final documentation and unsafe fingerprint refresh do not affect compiled code. Source replay and the compiled-source hashes validate. Build with `CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=benchmarks/rpc/target cargo build --locked --release --manifest-path benchmarks/rpc-compio/Cargo.toml`; select `CAPNTPROTO_BENCH_RUNTIME=tokio` on both peers. This uses the default Tokio-only harness; Compio remains opt-in.

Before SHA-256: `80f8ec7c1a90b7c9e4bd301c172636c5cd98f3cf02ed218524ffe7decd78ef6c`. After SHA-256: `d2e80c3501daba3ab5168e6a87be5e10a925c8347bf4ba64b2acea6faa122df5`. The report records reference hashes, the C++ source revision and fixture hash. Raw profiles, samples, decoded/original traces, both experiment sources and binaries, scripts and validation logs are retained. Earlier archives are unchanged.

Evidence: `target/portable-cpp-readers-20261017`, 2,097 files plus `SHA256SUMS`; manifest SHA-256 `7ea01c0fa775987d0534d683bbb6595a187e3dd8a0c7a90cb072f674452dcc71`.
