# Vendored rustls direct record output — 24 October 2026

The ciphertext copy can be removed. The local rustls 0.23.46 patch encrypts new
Unbuffered application records directly into `TlsStream`'s retained output
storage. This also removes one temporary allocation per outgoing TLS record.
Both legacy and portable benefit; the public transport API stays the same.

## Implementation and boundaries

The checksum-verified published crate is pinned at upstream commit
`f04804fa13341a9b1dc02b5d1dd8a746f2c4b7fb`. Its original notices and licenses remain.
[Provenance](../../../../vendor/rustls/UPSTREAM.json) hashes all 119 original files;
only five source files differ. The [local patch](../../../../vendor/rustls/LOCAL.patch)
is 172 lines. Its [maintenance notes](../../../../vendor/rustls/README.capntproto.md)
explain the entry points and tests.

`MessageEncrypter::encrypt_into` adds caller-owned output. Its default uses the
original owned-record method for other providers and TLS 1.2. The ring TLS 1.3
override writes the same record header, gathers plaintext into the destination,
uses the original `seal_in_place_separate_tag` AEAD operation and nonce derivation,
and places the tag in the final record. The record layer shares its existing
sequence/exhaustion logic. Whole-write size checks, fragmentation, queued-control
ordering and key refresh remain in `CommonState`.

This removes the application ciphertext allocation and copy, not every TLS copy.
Plaintext still enters output storage once. Queued handshake/control/alert records
retain their upstream path. Received plaintext still allocates. QUIC remains on
quiche's `Connection::stream_recv_buf()` and has no runtime change here.

The root dependency uses the vendored path, including when built by a downstream
workspace. Root and maintained RPC benchmark patches unify transitive rustls
users. The independent upstream fixture retains registry rustls. Framework gRPC
and buffered interoperability still use tokio-rustls; their buffered encryption
method is unchanged.

## Allocation and instruction result

| TLS bulk-1m, both peers | Instructions / operation | Allocations / operation | memcpy instructions / operation |
| --- | ---: | ---: | ---: |
| Before legacy | 20,234,769 | 275.00 | 8,650,378 |
| Direct-output legacy | 18,058,236 | 145.00 | 6,561,235 |
| Before portable | 20,280,004 | 286.00 | 8,707,913 |
| Direct-output portable | 18,103,575 | 156.00 | 6,611,698 |
| C++ | 19,375,790 | 2,356.00 | 4,389,926 |

The new path eliminates **130 outgoing-record allocations per operation**. Total
allocation blocks fall **47.3% for legacy and 45.5% for portable**. Both retain
130 receive-record allocations. Cumulative allocated bytes fall by about 2.10 MB
per round trip; this is allocation traffic, not a peak/live-memory measurement.

Simulated instructions fall **10.76% legacy / 10.73% portable**, and self
instructions attributed to memcpy fall **24.15% / 24.07%**. The new total is
**6.80% / 6.57% below C++** in this profiled bulk case. This does not establish
wall-clock speedup or an instruction advantage for every workload. The remaining
portable/legacy gap is mostly outside the shared TLS writer.

Profiles are paired 2/8-operation captures, one warmup each, summed over client
and service: `(long - short) / 6`. There are 20 Callgrind/DHAT trials, including
both frozen Rust controls and C++. Callgrind events are simulated, DHAT counts
cumulative allocation blocks, and instrumented timings are excluded from latency
results. [All events and hot functions](profile-comparison.json) are retained.
Conditional branch misses decrease, but indirect branch misses increase against
the Rust controls; this is not a reduction in every hardware-related metric.
Small negative cache/allocation-attribution slopes reflect startup subtraction
or floating-point roundoff. The C++ allocation slope is specific to this capture,
not substituted from the prior campaign.

## Repeated local latency and socket diagnostics

The [comparison table](comparison.md) contains nine TLS workloads, five variants,
five fresh-process repetitions, 50 warmups and 500 measured operations per run:
225 trials and 112,500 individual timings. Each cell is mean ± sample standard
deviation of five run means, with no trimming. Established-connection operations
include validation and cleanup; setup and handshakes are excluded. Concurrent
workloads time the whole batch. [Raw samples](latency-samples.json.gz) recompute
every table cell.

Portable's geometric mean latency is **4.30% lower** than its control, with lower
means in 6/9 cases. Legacy's is **0.44% lower**, with lower means in 4/9. Both have
lower local means than C++ in 9/9. These are noisy exploratory measurements:
legacy bulk-1m regresses from 1423.06 ± 109.44 to 1495.76 ± 167.30 µs, while
portable improves from 1541.78 ± 160.47 to 1425.56 ± 330.19 µs. Several small and
multiservice cases regress. The source and allocation proof is stronger than a
claim of consistent latency improvement.

[Successful ciphertext socket calls](io-comparison.md) and
[all attempts](io-attempt-comparison.md) use five paired 10/100-operation traces
with two warmups, summed over both peers. Dependent-read is 4 reads / 4 writes;
pipelined-read and concurrent-32 are 2 / 2 in both Rust drivers. Bulk-1m is
32.28 ± 6.62 reads / 18 writes for legacy and 35.17 ± 6.94 / 18 for portable,
versus 258.06 ± 0.09 / 258 for C++. The measured means meet or beat C++ in these
four cases. Read counts vary with socket scheduling; these samples do not prove
strict parity across every operation or untraced workload.

The first short trace ran while a read-only `callgrind_annotate` analysis was
finishing; the next phase's idle-host guard rejected that overlap before trials.
The final short and long captures ran after the analysis ended. The initial 100
short captures and guard-failure evidence are preserved separately and excluded
from the final tables. All 200 final trace trials passed validation.

## Validation and reproducibility

All **30 secure RPC tests and 35 selected native/identity tests pass**, as do
TLS-only compilation, all-target Clippy with warnings denied, and formatting.
The new differential test checks all three ring TLS 1.3 AEADs byte for byte
against the unchanged owned encrypter, and asserts zero direct-output allocations.
It covers empty/max-size input, scatter/gather ranges, short-buffer retries,
untouched output tails, content types, high sequence bits and fallback providers.
Real buffered/unbuffered interoperability forces automatic key rotations in both
directions for every cipher through small fragmented/backpressured transports.
Existing tests cover corruption, truncation, shutdown, certificate rejection,
ALPN and QUIC. [Commands/results](validation.json) identify the checks.

This is not a fresh full-project or full-upstream-rustls suite run. The published
rustls package omits upstream test-data fixtures; the repository differential and
integration tests validate this delta. Downstream/fuzz locked metadata and
maintained/independent fixture dependency trees were checked and preserved.

Implementation: `58205ddc5b3f0841848e836327b6c249d5200777`. Both controls are runtime
`e4b71e700f198f8da7ef7f02c99e2174b5e41013`, documented by `71f54ae`, with no vendor
patch. C++ is pinned to `0de72d8d8cec6b69edaa29de51d3bd490341f9c2`. Rust uses 1.97,
opt-level 3, fat LTO, one codegen unit, debug=1 and overflow checks. C++ uses
Clang 19, `-O3 -DNDEBUG`, CMake Release libraries and OpenSSL 3.5.7.
[Build provenance and binary hashes](provenance.json) identify exact executables.

The enclosing CPU quota is four CPU-equivalents, with affinity 0–3, client on 0,
services rotating over 1–3. The four-service fixture shares a service core equally
for every variant. No compiler/profiler ran during native timings. Outer counters
record zero throttled periods; maximum aggregate per-trial native CPU steal is
7.14%, and run-to-run spread is substantial. [Host counters](host-health.json)
retain the quota hidden by Docker's inner cgroup. Full profiles, traces, logs,
scripts and timing samples are indexed in [raw-artifact.json](raw-artifact.json).
Generated private certificate directories and binaries are excluded from that
archive; binary hashes are retained.

[Dedicated qualification 40062326199](https://github.com/DericHuynh/capntproto/actions/runs/40062326199)
uses workflow source `9ebdaf5ada63d399dca89cbdd53ec2bf3adf6dff`, freezes both Rust
controls at `71f54ae`, and waits for qualification 40059426498 to acquire the
single dedicated measurement slot before it builds and queues. Results are
pending. This local bulk result does not close the full dedicated-host allocation
or performance gap.
