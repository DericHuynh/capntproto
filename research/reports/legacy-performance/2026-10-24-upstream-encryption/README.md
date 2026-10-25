# Upstream rustls encryption and direct RPC frames — 24 October 2026

The vendored rustls implementation now follows upstream PR #3127's caller-owned
ciphertext contract. A subsequent protocol change sends contiguous RPC frames
straight to the scalar transport writer, removing their intermediate TLS gather
copy and the custom one-frame write loop. Each step has its own frozen control,
five-repeat local comparison, and instruction/allocation profiles.

## Upstream alignment

[PR #3127](https://github.com/rustls/rustls/pull/3127), merged at
`73a793c90f2e8c8b651cd3f1131f4be27f41259b`, already provides this capability upstream.
No upstream issue or pull request was created. The local change is a targeted
backport onto the checksum-verified 0.23.46 source, not a complete 0.24 upgrade.

The bespoke `encrypt_into` method is removed. `MessageEncrypter::encrypt` takes
caller-owned output and returns borrowed `EncodedMessage` metadata. The record
layer writes the TLS header after the provider encrypts its payload. All seven
ring/AWS-LC TLS 1.2/1.3 implementations match upstream after the documented 0.23
input-type and nonce adaptations. The upstream `EncryptBuffer` and chunk iterator
are adapted to the existing error type and compiler-support policy. The 0.23
Unbuffered state machine, size preflight, record sequencing, queued-control order
and key updates remain. The complete 0.24 split-state API and output recycler are
outside this backport.

**The crypto-provider trait API changes:** custom encrypters must implement the
new borrowed-output contract; the owned-record fallback is gone. Capntproto's
application TLS constructor/configuration signatures stay the same. See
[vendor maintenance notes](../../../../vendor/rustls/README.capntproto.md),
[upstream provenance](../../../../vendor/rustls/UPSTREAM-3127.json), and
[provider equivalence checks](upstream-tests/provider-equivalence.json).

The [alignment profiles](alignment/profile-comparison.json) retain the preceding
patch's allocation counts: unary 7 legacy / 10 portable, pipeline 58 / 67, and
bulk-1m 145 / 156 blocks per operation. Instructions are close to the preceding
implementation. The [alignment timing table](alignment/comparison.md) is mixed:
legacy's geometric mean is 4.94% lower, portable's 2.45% higher. This is a
consistency change with preserved allocation behavior, not a uniform speedup.

## Protocol simplification and measured result

`write_framed` now delegates to the standard scalar `write_all`. A batch with one
remaining contiguous slice uses that same path. TLS encrypts directly from that
slice instead of gathering it into its retained plaintext scratch buffer first.
Multi-slice writes keep scatter/gather batching. Frame bytes, ownership through
partial writes, Pending, write errors and flush completion keep their existing
contracts. The shared serializer also serves TCP and QUIC; only TLS is locally
benchmarked here. QUIC reception still uses `Connection::stream_recv_buf()`.

The [protocol profile table](protocol-profile.md) compares against the frozen
upstream-aligned binary. Unary instructions fall 0.12% for legacy and 0.37% for
portable; portable unary memcpy self-instructions fall about 4%. Bulk instructions
change by less than 0.04%, and portable pipeline instructions rise about 0.01%.
Allocation counts are unchanged. These small effects do not establish a broad
performance gain. Simulated counters and startup-subtracted slopes vary with
execution paths and scheduling; all events and top allocation stacks are saved.

The [final timing table](comparison.md) has nine cases, five variants and five
fresh-process repetitions, with 50 warmups and 500 measured operations per run.
Cells report mean ± sample SD of five run means; no samples are trimmed. Handshake
and setup are excluded; validation/cleanup are included, and concurrent fixtures
time the entire batch. Both stages retain every individual timing sample.

The scalar-path step has geometric mean latency **3.88% higher for legacy and
0.79% higher for portable**, with lower means in 4/9 and 2/9 cases respectively.
Both final drivers have lower local means than C++ in 9/9 cases. The measurements
are noisy and do not demonstrate a latency improvement from this step. For
example, legacy unary is 31.61 ± 6.16 versus 25.91 ± 0.77 microseconds, while
portable bulk-64k is 97.99 ± 2.45 versus 115.25 ± 20.47 microseconds. Retaining the
simpler path is supported by eliminating redundant work, not by claiming a
consistent measured speedup. Dedicated qualification remains necessary.

[Successful socket calls](io-comparison.md) use five paired 10/100-operation
traces with two warmups, subtracting fixed setup and summing both peers. Rust
reads/writes are 4/4 for dependent-read and 2/2 for pipeline and concurrent-32,
matching C++. Bulk-1m is 32.12 ± 3.94 reads / 18 writes for legacy and
30.05 ± 3.20 / 18 for portable, versus about 258.01 / 258.01 for C++.
[All-attempt counts](io-attempt-comparison.json) also retain unsuccessful calls.
Small fractional slopes reflect setup subtraction; reads depend on scheduling.
These four cases do not prove strict parity in all workloads.

## Remaining gaps

Portable pipeline still allocates 67 blocks per operation versus legacy's 58.
Eight portable blocks create shared reader arenas. `Frame::with` and `with_size`
promote ordinary reads to shared ownership; the allocation trace includes one
from `EncodedCall::validate_promised`, a validation-only use. Separating temporary
inspection from retained payload ownership is a concrete next target. It must
preserve the same traversal allowance and pointer identity when a payload really
escapes; simply opening a fresh reader would reset budgets incorrectly. This
report does not claim all eight allocations can be removed.

Both Rust drivers also retain 130 received-plaintext record allocations per
bulk-1m operation. The outgoing ciphertext copy and allocation are gone, but the
0.23 receive ownership path remains. These protocol/receive issues are larger
than the scalar writer's measured benefit. The full dedicated-host allocation
and latency gaps remain open.

## Validation and reproducibility

The exact upstream 0.23 workspace, with provider-test plumbing adapted for the
new contract, passes **543 ring, 549 default AWS-LC and 950 all-non-FIPS test
executions**, including unit, integration and doc tests. Every matrix run retains
eight upstream-ignored cases. BoGo, FIPS and the complete platform/toolchain matrix
were not run. [Gzipped logs and commands](upstream-tests/checks.json) are retained; the
[test adaptation patch](../../../../vendor/rustls/UPSTREAM-TESTS.patch) changes
provider plumbing, not protocol assertions.

The protocol step passes **77 serialization/write-queue, 30 secure RPC and 35
selected native/identity tests**, TLS-only compilation, all-target Clippy with
warnings denied for the root and serializer packages, and formatting. Existing
checks cover partial/Pending/error/zero writes, ownership and flush completion,
TLS interoperability, all three TLS 1.3 AEADs, short buffers, zero direct-record
allocations, key rotation, and QUIC. This is not a fresh full-project test run.
[Validation commands and logs](validation/checks.json) identify the exact scope.

Alignment commit: `0b6eacf478086e63832baf8fe5ad6cd184a3ce4c`; protocol commit:
`36318e33144844d855b19f364d0e8d154fd1527d`. Alignment controls use runtime `58205dd`
(documented by `5b8fef2`); protocol controls use `0b6eacf`. C++ remains pinned to
`0de72d8d8cec6b69edaa29de51d3bd490341f9c2`. [Provenance](provenance.json) records
binary hashes and Rust 1.97 opt-3/fat-LTO/one-CGU/debug-1/overflow-check builds,
plus the Clang 19/OpenSSL 3.5.7 C++ build. The first alignment link was killed by
memory pressure; retry used identical flags after reclaiming verified duplicate
artifacts. No compiler/profiler ran during native timing.

Both stages use four CPU-equivalents and affinity 0–3, client on 0 and services on
1–3. The four-service case shares a service core equally across variants. Neither
native phase had outer-cgroup throttling. Maximum aggregate per-trial CPU steal
was 7.69% for alignment and 9.52% for the protocol step. [Host counters](host-health.json)
and [alignment counters](alignment/host-health.json) retain this limitation.
The [raw artifact index](raw-artifact.json) covers both stages' profiles, traces,
timings, scripts, build/test logs and outer counters, excluding executables and
generated certificate/key directories. Its archive was verified file by file.

[Dedicated qualification 40070062202](https://github.com/DericHuynh/capntproto/actions/runs/40070062202)
uses workflow source `120118c3fab9b967d1648b84fd1b367b35554e05`, whose runtime parent
is `36318e3`. Both Rust controls are frozen at `5b8fef2` (runtime `58205dd`). Its
guard waits for run 40062326199 to acquire the dedicated measurement slot or
finish successfully before the new run builds and queues. Results are pending.
