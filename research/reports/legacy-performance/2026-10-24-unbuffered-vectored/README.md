# Unbuffered TLS: trace-led vectored writes — 24 October 2026

The TLS adapter now accepts vectored plaintext directly. Large contiguous payloads
go directly to rustls; fragments are combined one TLS record at a time. Ciphertext
is batched for up to 128 KiB of plaintext per acceptance, plus record overhead. TLS RPC no longer stages entire
128 KiB plaintext batches in the outer Tokio adapter. A short path handles batches
that fit into one record without traversing the general cross-record cursor.

Both legacy and portable use this implementation. Rustls remains on its Unbuffered
API; QUIC remains on `Connection::stream_recv_buf()`. No crypto implementation,
authentication policy, TLS version, dependency pin, or benchmark payload changed.
The public addition is standard `AsyncWrite` vectored-write support.

## What the profiles found

Before optimization, the API switch raised portable's simulated instructions for
TLS bulk-1m from 18.296 million (buffered) to 20.483 million (Unbuffered), while
reducing cumulative allocations from about 344.7 to 286 per round trip. Nearly all
of that instruction increase was in memory-copy routines. The old buffered
adapter grew and shrank rustls's deframer; retaining input storage removed that
allocation churn. Rustls 0.23.46 still allocates every decrypted and encrypted
record, and its Unbuffered `write_fragments()` copies each encoded ciphertext
record into the caller's output slice. The API is not an allocation-free or
ownership-transferring record interface.

The first optimization removed the outer large plaintext copy. Its bulk profiles
showed fewer instructions but a small instruction increase for tiny writes, so a
second change added the one-record short path. Both steps were built and measured
separately. [Step-one timings](step1-comparison.md) and all raw samples are retained.

| TLS bulk-1m, both peers | Instructions / operation | Allocations / operation | Copy instructions / operation |
| --- | ---: | ---: | ---: |
| Pre-switch portable | 18,295,996 | 344.67 | 6,706,884 |
| Unbuffered portable before optimization | 20,483,476 | 286.00 | 8,916,892 |
| Portable after step one | 20,278,461 | 286.00 | 8,708,006 |
| Final portable | 20,274,930 | 286.00 | 8,708,203 |
| Unbuffered legacy before optimization | 20,482,529 | 275.00 | 8,910,185 |
| Final legacy | 20,229,083 | 275.00 | 8,650,438 |
| C++ | 19,382,408 | 2,340.67 | 4,389,927 |

Final bulk instruction counts fall **1.24% for legacy and 1.02% for portable**
against the prior Unbuffered build. Allocations remain unchanged at 275 and 286.
The short path reduces the small-write overhead from step one, but does not remove
it entirely: the profiled small cases still use 0.09–0.56% more instructions than
the previous adapter. This is a tradeoff, not an across-the-board reduction.

These are diagnostic slopes, `(8-iteration profile - 2-iteration profile) / 6`, with one warmup at both
points, summed over client and service. Callgrind instruction/cache/branch values
are simulated; DHAT measures cumulative allocation blocks. Instrumented latency
is never used in the timing tables. This is one paired profile per variant, not
five repeated timing trials. Small startup-sensitive cache deltas can be negative.
Full events, allocation categories and hot functions are retained in
[profile-comparison.json](profile-comparison.json).

| Small-call instruction check | Before | Step one | Final | Final change |
| --- | ---: | ---: | ---: | ---: |
| unary-small, legacy | 46,456 | 46,893 | 46,714 | +0.56% |
| unary-small, portable | 47,129 | 47,426 | 47,329 | +0.43% |
| pipelined-read, legacy | 97,106 | 97,571 | 97,297 | +0.20% |
| pipelined-read, portable | 147,690 | 148,029 | 147,822 | +0.09% |

Small-call profiles use paired 10/50-iteration captures with two warmups. The
[complete small-call events](small-profile-comparison.json) include C++ and
conditional/indirect branch misses. The tiny-write path is checked independently
because a large-payload win does not establish a small-call win.

## Local latency and socket counts

The [final comparison table](comparison.md) covers nine TLS workloads, all five
variants, five fresh-process repetitions, 50 warmups and 500 measured operations
per repetition: 225 trials and 112,500 timings. Mean and sample SD of run means
are retained, with no trimming. Setup/handshakes are excluded; validation and
cleanup remain timed. Concurrent workloads measure the entire batch. The first
step has another 225 trials and 112,500 retained timings.

- Legacy: -0.82% geometric mean latency versus its previous Unbuffered build; lower mean in 6/9 cases. Lower mean than C++ in 9/9 cases.
- Portable: -1.01% geometric mean latency versus its previous Unbuffered build; lower mean in 6/9 cases. Lower mean than C++ in 9/9 cases.

These are exploratory local means, not proof of uniform speedup or dedicated-host
qualification. The full table includes regressions and between-run spread.
[Successful syscall counts](io-comparison.md) and [all-attempt counts](io-attempt-comparison.json)
come from five paired 10/100-iteration traces per case with identical two-operation
warmup. They cover dependent reads, pipelined reads, concurrent-32 and bulk-1m,
including both Rust controls and C++. The diagnostic timings are excluded.

Final bulk-1m successful reads/writes are **32.24 / 18 legacy**, **28.49 / 18
portable**, versus **258.00 / 258.00 C++**. Small-case estimates are approximately
4/4 for dependent reads and 2/2 for pipelined and concurrent calls. One-call
variation between paired captures produces small departures: dependent C++ reads
are 3.998 versus Rust's 4.000, and legacy pipelined reads are 2.002 versus C++'s
2.000. These estimates do not prove strict successful-read parity. All-attempt
read/write slopes meet or beat C++ in the four traced final cases; no claim is made
for the untraced cases. Bulk reads also vary with socket scheduling and did not
uniformly improve against the prior Rust build.

## Remaining limits

The unavoidable copy through the pinned Unbuffered output API remains. Eliminating
it requires upstream record/crypto API support or maintaining a dependency patch;
this change does neither. Current receive records also retain rustls's internal
allocation. The local bulk profiles therefore do not establish instruction parity
with the old buffered path, even though they retain its allocation improvement
and use many fewer socket calls than C++.

The portable/legacy difference for short dependent and pipelined calls remains a
separate RPC-driver cost: both now use the same TLS adapter. A common transport
optimization does not remove that difference. QUIC had no runtime change in this
optimization; the pinned quiche's slice `stream_recv()` already delegates to
`stream_recv_buf()`, whose BufMut interface initializes received bytes but does
not transfer ownership of reassembly storage. No new QUIC latency claim is made.

## Validation, provenance and host limits

All 28 secure RPC tests and 35 selected native/identity tests passed. The new test
covers empty vectors, record boundaries, partial acceptance, bidirectional
backpressure, independent reader/writer wakers, and 32-byte fragment configuration.
Each side starts reading after its own handshake so post-handshake tickets can
drain through the tiny transport. TLS-only compilation and all-target Clippy with
warnings denied passed. [Validation commands](validation.json) identify the exact
checks; this is not a claim of a fresh full-project test run.

Implementation commit: `e4b71e700f198f8da7ef7f02c99e2174b5e41013`. The local post-switch control was built from `ded9a9536ed317109ee90ce4e399889a5607a67c`, runtime `bd60556f1d48fd38355fa0c17f4f304cec444bf8`. Dedicated qualification freezes `34b5cdabf2201d0b50f03ec6dc18b35e8a9f1807`, which adds only the earlier local report to that runtime. Pre-switch diagnostic control: `867fb7863db47e559de521f90b743769fd390a68`. C++: pinned `0de72d8d8cec6b69edaa29de51d3bd490341f9c2`.

Rust builds use the same Rust 1.97 release profile: opt-level 3, fat LTO, one
codegen unit, debug=1 and overflow checks. C++ uses Clang 19, `-O3 -DNDEBUG`, pinned
CMake Release libraries and OpenSSL 3.5.7. Binary hashes are in
[provenance.json](provenance.json). The first final-release link was killed by the
16 GiB environment memory limit; validation completed, the rebuildable debug cache
was compressed, and the release build was retried without changing its profile.

The enclosing CPU quota is four CPU-equivalents; affinity is restricted to cores
0–3, client on 0 and services rotating over 1–3. Four-service cases share one
service core identically across variants. Native trials run without compilers or
profilers. [Host counters](host-health.json) retain the outer cgroup quota and
throttling, because the Docker-visible cgroup hides that limit.

- step1 native phase: 0 throttled periods, 0 throttled µs; maximum aggregate per-trial CPU steal 8.33%.
- final native phase: 0 throttled periods, 0 throttled µs; maximum aggregate per-trial CPU steal 8.33%.

Raw timings are retained in [latency-samples.json.gz](latency-samples.json.gz) and
[step1-latency-samples.json.gz](step1-latency-samples.json.gz); all table cells are
recomputed from those samples. Full profiles, per-peer syscall traces, commands,
scopes and validation logs are identified by [raw-artifact.json](raw-artifact.json).

[Dedicated qualification 40059426498](https://github.com/DericHuynh/capntproto/actions/runs/40059426498) uses workflow source `878282b95eb7561856805a23ab31d85a0eb39db9` with frozen legacy and portable controls. Its guard waits for qualification 40046022437 to acquire the dedicated measurement slot before building and queuing, preserving GitHub's single pending measurement job. Results are pending.

## Follow-up: direct ciphertext output

The [vendored rustls follow-up](../2026-10-24-rustls-direct-output/README.md) removes
the remaining application-record ciphertext allocation and copy described above.
Bulk-1m allocations fall from 275 to 145 for legacy and 286 to 156 for portable;
simulated instructions fall about 10.7%. Receive-record allocations remain. Local
latency is mixed and dedicated qualification 40062326199 is pending.
