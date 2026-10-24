# Local Unbuffered / BufMut comparison — 24 October 2026

**The new transport path uses fewer socket reads/writes, but the local TLS
latency results show regressions, particularly for large payloads. It is not
qualified as a uniform performance improvement.**

These local runs were explicitly requested after the earlier CI-only constraint.
They are separate from the dedicated droplet campaigns. No production code was
changed during this comparison.

Candidate source: `ded9a9536ed317109ee90ce4e399889a5607a67c`, implementation
`bd60556f1d48fd38355fa0c17f4f304cec444bf8`.
Frozen pre-switch source: `867fb7863db47e559de521f90b743769fd390a68`.
C++ source: pinned `0de72d8d8cec6b69edaa29de51d3bd490341f9c2`, checked against
its source manifest. Both Rust revisions were freshly built with Rust 1.97,
release optimization, fat LTO, one codegen unit, debug=1 and overflow checks.
C++ was rebuilt with Clang 19, `-O3 -DNDEBUG`, and pinned CMake Release libraries.
Local OpenSSL is 3.5.7. Binary hashes and build details are in [provenance.json](provenance.json).

## Timing results

The [full comparison](comparison.md) retains legacy, portable, C++ and both
frozen Rust controls, including means, run SDs, and changes from each control.
There are nine workloads on each transport: small unary calls, dependent reads,
pipelined reads, 64 KiB/1 MiB/4 MiB transfers, depth-32 access, concurrent-32,
and a four-service/depth-16/eight-client concurrent chain.

Every cell has five fresh-process repetitions, each with 50 warmups and 200
retained operations. There are 405 main timing trials, preceded by 81 successful
smoke trials. The concurrent cases measure a whole 32-call batch. Setup and TLS
handshakes are excluded; request construction, response validation and cleanup
remain inside timing. No samples were trimmed.

| Transport | Driver | Mean latency vs previous version, geometric mean across 9 cases | Lower mean than previous | Lower mean than C++ |
| --- | --- | ---: | ---: | ---: |
| TLS | Legacy | +6.1% | 5/9 | 9/9 |
| TLS | Portable | +11.8% | 1/9 | 8/9 |
| QUIC | Legacy | -3.2% | 6/9 | unsupported |
| QUIC | Portable | +2.6% | 3/9 | unsupported |

These counts describe sample means, not statistical significance or acceptance
of the complete benchmark suite. In particular, portable's TLS pipelined-read
mean of 80.36 ± 21.22 µs overlaps the C++ mean of 77.85 ± 10.62 µs substantially.
QUIC changes are mixed; this run does not establish a consistent QUIC speedup.

The first TLS bulk-1m comparison showed +38.8% legacy and +43.6% portable mean
latency versus their frozen controls. Bulk-4m showed +18.8% and +22.2% respectively.
A separate, larger bulk-1m repeat used five fresh processes per variant, 100
warmups and **1,000 measured operations per process**:

| TLS bulk-1m | Frozen control, µs | New path, µs | Mean change |
| --- | ---: | ---: | ---: |
| Legacy | 1211.48 ± 411.05 | 1392.26 ± 277.78 | +14.9% |
| Portable | 984.07 ± 212.52 | 1407.43 ± 137.72 | +43.0% |
| C++ reference | — | 2473.20 ± 460.98 | — |

All ± values are sample standard deviations of the five repetition means,
not confidence intervals. The larger repeat still has substantial variation,
especially in the legacy control; it does not establish an exact stable slowdown.
Both complete timing datasets, including the slower samples, are preserved in
[latency-samples.json.gz](latency-samples.json.gz): 430 trials and 106,000 timings.

## Socket reads and writes

The [syscall comparison](io-comparison.md) covers all five variants using
five paired 10/100-iteration traces per case, each with two warmups: 200 diagnostic
trials. Counts are successful ciphertext socket syscalls summed over client and
service. Subtracting short from long and dividing by 90 removes most fixed
setup/shutdown cost. These instrumented timings do not enter the timing tables.

| TLS case | New legacy reads/writes | New portable reads/writes | Frozen legacy reads/writes | Frozen portable reads/writes | C++ reads/writes |
| --- | ---: | ---: | ---: | ---: | ---: |
| Dependent read | 4 / 4 | 4 / 4 | 4 / 4 | 4 / 4 | 4.002 / 4 |
| Pipelined read | 2 / 2 | 2 / 2 | 2 / 2 | 2 / 2 | 2 / 2 |
| Concurrent-32 | 2 / 2 | 2 / 2 | 4 / 2 | 4 / 2 | 2 / 2 |
| Bulk-1m | 35.53 / 18 | 29.55 / 18 | 140.81 / 34 | 137.24 / 34 | 258.00 / 258.00 |

The formerly excessive concurrent-32 receive count reaches C++ parity in these
captures. The bulk path reduces both counts substantially. All four traced
current Rust cases are at or below C++ in estimated successful reads and writes.
[All-attempt counts](io-attempt-comparison.json), including would-block, errors,
zero returns and unfinished calls, also meet or beat C++ in these four cases.
C++ attempted reads per operation are 8.004, 4.000, 3.998 and 283.369 respectively;
the current Rust attempted-read slopes equal their successful-read slopes in these
captures. Attempted-write slopes likewise equal the successful-write slopes.
This does not establish parity for untraced cases, protocol-message counts, or
allocations. Per-repetition spread and original per-peer counts are retained
alongside the comparison.

The bulk latency regression is therefore present despite lower syscall counts.
The pinned rustls Unbuffered implementation allocates/encodes ciphertext records
and copies them into its caller's output slice, while the buffered path can send
its queued encrypted chunks directly. The new scalar TLS writer also selects
the adapter's bounded plaintext coalescing path for large vectored frames.
Those copy passes and per-record delivery are the next profiling targets; these
socket traces alone do not quantify their CPU cost or establish causality.

## Host limits and evidence

The enclosing environment allows four CPU-equivalents across five available
cores. The runner was restricted to cores 0–3: client on 0, services cycling over
1–3. The four-service case consequently shares one service core, identically
for every variant. Both builds completed before timing began.

Docker's visible cgroup reports no quota, so a separate outer-host monitor was
required. It recorded **zero throttled periods and zero throttled microseconds**
in both uninstrumented phases and both trace phases. This does not eliminate
host scheduling noise: the maximum aggregate CPU steal delta around a main trial
was 11.8%, and several cells have large between-run spread. Results are local
exploration, not replacement evidence for dedicated-host qualification.

- [Host counters](host-health.json), [main scope](native-scope.json), and
  [larger-repeat scope](tls-bulk-confirm-scope.json) retain sampling/placement.
- [Compact results](comparison.json), [larger repeat](bulk-confirmation.json),
  [short trace counts](trace-short-counts.json) and [long trace counts](trace-long-counts.json)
  retain all measurements used in the tables.
- [Raw archive manifest](raw-artifact.json) identifies the 67,676,451-byte local
  archive containing all process logs and syscall traces. SHA-256:
  `b26ebfed9a872ec428fd777119d10edfefa5ea831f13921625d2938d21d77682`.

The [dedicated Unbuffered qualification](https://github.com/DericHuynh/capntproto/actions/runs/40046022437)
initially stopped when the preceding run was cancelled. Its failed job was
rerun after that preceding qualification restarted; the guard has now passed
and the dedicated benchmark bundle is building. No new droplet results are
claimed here, and the allocation-parity gaps remain open.
