# Legacy allocation and concurrency comparison, October 23, 2026

The final candidate removes about **4% of cumulative allocations** in the
profiled concurrent TLS workloads and fixes a reproduced cross-thread wake bug.
Across all 24 cases, geometric mean latency is **1.12% lower on TLS** and
**0.58% higher on QUIC** than frozen legacy. It is not faster in every case.
All performance evidence comes from CI DigitalOcean droplets; local runs are
correctness checks or analysis of saved artifacts.

## Final comparison

[Run 39976113886](https://github.com/DericHuynh/capntproto/actions/runs/39976113886)
measured `8effa2ab8f7af1b94b6d200482035c99077d90b4`, including the duplicate-wake
publication fix. All **840 native trials**, **350 syscall-trace trials**, and
**64 Callgrind trials** completed. DHAT completed 58/64 trials; both operation
counts for the four-service QUIC fixture disconnected for candidate legacy,
frozen legacy and portable. Consequently **the overall CI run failed**, and
full profiling qualification remains open. Analysis succeeded, both droplets
were destroyed, and the cleanup verification step passed.

See [all final comparison tables](final-tables.md), [the final machine-readable
summary](final-summary.json), [CI step outcomes](final-ci.json), and [failed
profile client logs](profile-failures.json). The tables retain mean ± sample SD
for every native cell, supported allocation/instruction comparisons, positive
socket calls, all attempts, and complete capture totals. No missing DHAT pair is
treated as a zero allocation count.

| Comparison | TLS | QUIC |
| --- | ---: | ---: |
| Legacy latency change vs frozen legacy, geometric mean | -1.12% | +0.58% |
| Cases with lower legacy mean vs frozen legacy | 16/24 | 15/24 |
| Legacy latency change vs current portable, geometric mean | -5.03% | -4.52% |
| Cases with lower legacy mean vs current portable | 21/24 | 15/24 |
| Legacy latency change vs C++, geometric mean | -38.80% | Unsupported |
| Cases with lower legacy mean vs C++ | 24/24 | Unsupported |

| TLS workload | Frozen allocations | Final allocations | Reduction |
| --- | ---: | ---: | ---: |
| Dependent read | 120 | 118 | 1.67% |
| Pipelined read | 137 | 135 | 1.46% |
| 32 concurrent calls | 786 | 754 | 4.07% |
| Depth 32, 32 concurrent calls | 802 | 770 | 3.99% |
| Four services, eight clients | 4754.42 | 4564 | 4.01% |

These counts are per complete fixture operation or concurrent batch, over every
peer. Fewer blocks does not imply proportionally fewer bytes: four-service TLS
cumulative bytes increased by 0.18% in this short/long profile subtraction.
Instructions fell only 0.05–0.70% in the TLS profiles. Eight DHAT workload pairs
have complete candidate/control comparisons; the ninth, four-service QUIC, has
none in this run. Its Callgrind and native comparisons are complete.

Spread remains material. TLS concurrent-32 changed from 206.087 ± 78.812 to
179.754 ± 42.646 microseconds, and deep concurrent access from 594.943 ± 167.916
to 545.916 ± 115.514. Four-service TLS changed from 2483.992 ± 62.866 to
2448.033 ± 70.635. Those observed improvements do not justify a uniform latency
claim. Regressions include TLS unary-small (+10.67%), QUIC depth-32 concurrent
access (+30.31%), QUIC deep-32 (+27.68%), and QUIC write-read (+19.73%). All five
repetition means remain available in the JSON, including the unfavorable runs.

This rerun used a different droplet from the first campaign. Compare each
candidate only to its same-campaign frozen control; do not pool the two runs or
attribute their difference solely to the publication fix. The frozen legacy
binary SHA-256 is identical in both campaigns. The final host recorded no CPU
throttling; maximum sampled single-CPU steal was 4%, with substantial latency
spread still visible in several workloads.

All six failed DHAT clients report a remote disconnect, including the unchanged
frozen legacy executable. Native, traced and Callgrind runs of this fixture
completed. The ten-second QUIC idle timeout and instrumented startup remain a
plausible explanation, but the saved logs do not expose the originating
transport error. No timeout or TLS parameter was changed to make the profiles
pass. The next diagnostic step is to expose the transport close reason and
separate setup deadlines from instrumented service-chain startup, identically
for all implementations, before collecting the missing allocation comparison.

The raw artifact is ID `12516022039`, 302,567,481 bytes, SHA-256
`c8e3cdab639fdebc2e34f5fdeccd4bf6fa0eaa2e50fc68cec8688cd0e23a3d44`.
The analysis artifact is ID `12515966949`, 30,293,490 bytes, SHA-256
`20c21a6b5e0debafbd4e52cea8fd79239c2cef7296185164f573ac05b44d199d`.
Both downloaded archives match GitHub's artifact digests. Raw data includes
native samples, complete socket traces, every peer profile, failed-trial logs,
CPU topology, executable hashes and per-trial host counters.

## Implementation

- Reuse each small-task slot's notification node across completed futures. The
  existing bounded atomic FIFO still coalesces cross-thread wakes and preserves
  wake order. Reuse also lets downstream registrations recognize the same waker.
  A stale wake can cause an extra poll, but cannot complete or discard a new
  future. Completed futures are dropped immediately; cached notifications retain
  neither futures nor the parent executor after teardown. Retention is bounded
  to eight notification nodes per task-set lane.
- Share the legacy cancellation context directly with dispatch. The retained
  results owner is created on demand and carries the existing return guard.
  This removes the separately boxed pair of reference-counted handles on normal
  dispatch. Results abandoned by a protected method remain alive until its last
  guard drops, and cancellation still emits one Return.
- Publish duplicate task notifications with a release atomic operation, even
  when the task is already queued. Previously, the producer could store data,
  observe an existing notification and return, while the consumer removed that
  notification and observed the old data. The actual Rust regression test failed
  at schedule 3,001 before the fix; it now passes 100,000 schedules. The successful
  CAS either publishes to the consuming poll or retries after dequeue and queues
  another notification. This is a correctness check, not a local performance run.
  The defect predates notification reuse; this is not evidence that it caused the
  separate portable QUIC profiling disconnect.

The protocol return owner and retained-results owner retain distinct lifetimes.
Blindly folding both into one owner would couple result capability retention to
pipeline-only bookkeeping. The implementation preserves that separation while
removing the dispatch box.

## Concurrency architecture

Legacy's admission queue uses local `Rc<RefCell<VecDeque<_>>>` state and ordinary
completion channels use one local `Rc` allocation. These paths do not have a
mutex to replace. Cross-thread task wakes already use an atomic FIFO for small
sets; the larger `FuturesUnordered` path already uses an atomic readiness queue.
A general MPMC queue would add synchronization to single-thread admission.
The fixtures exercise concurrent in-flight calls and multiple connections on
local executors; they do not have multiple threads mutating the same RPC tables.
The local transport FIFO also uses `Rc`/`RefCell`, but its exported
`OutgoingQueue` diagnostics still use `Arc<Mutex<Metrics>>` for cross-thread
snapshots. That is real synchronization in the local write path; the campaign
has no demonstrated contention on it. Splitting local metrics ownership from an
optional cross-thread observer is another specific API simplification to test,
rather than replacing the existing local FIFO with an MPMC queue.

The scheduler helper is shared with portable and flow-control task sets; the
measured portable column therefore uses the current candidate binary. Only legacy
has a frozen control in this experiment.

The remaining scheduler limitation is allocation reuse above eight live slots.
Promotion to `FuturesUnordered` is permanent, and its task nodes are separately
allocated. The pending sentinel also occupies one small slot. Pending protected methods
still fork a shared future, and ordinary outgoing questions still allocate a
separate completion channel beside their reference owner. Those are ownership
paths to attribute before replacing more scheduler machinery. The new profiles
include 32-way concurrency, depth-32 concurrent access, and four services with
eight clients to distinguish this cost from transport and serialization work.
This change does not claim allocation-free scheduling at arbitrary concurrency.

The Rust concurrency fixtures also use `try_join_all`, which creates its own
future-queue nodes for a 32-call batch. Those application-level allocations must
be attributed separately from `TaskSet` nodes. C++ uses KJ `joinPromises` for the
same batch contract. These fixture costs remain included in the measurements;
the timed fixture code is unchanged between the two legacy revisions.

[KJ's TaskSet](https://github.com/capnproto/capnproto/blob/0de72d8d8cec6b69edaa29de51d3bd490341f9c2/c%2B%2B/src/kj/async.c%2B%2B#L265) attaches its bookkeeping to promise storage with `appendPromise`,
and [RpcCallContext](https://github.com/capnproto/capnproto/blob/0de72d8d8cec6b69edaa29de51d3bd490341f9c2/c%2B%2B/src/capnp/rpc.c%2B%2B#L3159) owns cancellation and result state. These are useful ownership
comparisons; copying an intrusive C++ pointer layout into Rust is not required to
remove the redundant box or reuse a wake node. TLS/QUIC dependencies and general
shared futures still contain synchronization; this is not a claim that the whole
RPC stack is lock-free.

## Measurement contract

The first measured source is `1241de15a070a06797e9e53e433562c3861da3a2` (runtime change
`f0a5e22`). The final runtime includes publication fix `5ee632f` and was measured
from `8effa2ab8f7af1b94b6d200482035c99077d90b4`, with frozen legacy control
`58c4dc7ace411f48ae90a580d162633d95eccbf7`. CI builds both with the same Rust
compiler and release profile, then interleaves them with current portable and
C++ on dedicated DigitalOcean vCPUs. Established connections use authenticated
TLS 1.3 or QUIC v1; pinned C++ has no QUIC implementation.

All 24 cases have five independent process repetitions, 100 warmup operations
and 1,000 measured operations. Means, sample standard deviations of repetition
means, and ranges are retained. Diagnostics run only after every native trial.
Callgrind counts simulated instructions/branches; DHAT counts cumulative
allocations. Short/long subtraction uses the points recorded for each workload:
50/500 for reads and 5/50 for concurrent batches, with ten warmups in each run.
Totals include the client and every service process, not only the first service.
The four-vCPU droplet pins the client to CPU 0 and rotates service hops over
CPUs 1–3; in a four-service chain the first and last services share CPU 1. Every
implementation uses the same placement. Five repetitions rotate variant order
but cannot exactly balance all positions across three or four variants.
Counts are per complete fixture operation; a concurrent operation contains
32 root calls, with additional calls at downstream service hops.

## Correctness

- 361 RPC unit tests pass, including cross-thread notification, FIFO, stale-slot,
  parent replacement, promotion, teardown and retained-result ownership checks.
- 25 droplet orchestration/cleanup tests and 11 benchmark analysis tests pass.
- 25 cancellation, static cancellation, admission and reply integration tests
  pass, including four model-trace replay tests.
- Rust formatting, Python syntax, workflow YAML parsing and patch whitespace
  checks pass. RPC Clippy also passes with warnings denied. Full coverage and hardware PMU sampling are not rerun.

The 422 selected local tests pass. CI additionally passed its 20 fixture tests,
25 cloud tests, 11 Python tests, workflow smoke checks and TLS certificate
rejection checks before measuring. The final CI failure is the six DHAT trials
described above, not a green overall pipeline.

The first local unit build was killed under memory pressure; removing only
rebuildable release artifacts allowed the same checks to pass. The first
integration invocation lacked `TLA2TOOLS_JAR`; rerunning with the existing pinned
1.7.4 JAR (verified against the repository digest) passed all selected tests.

## First campaign: allocation improvement, latency gap remains

[Run 39963907209](https://github.com/DericHuynh/capntproto/actions/runs/39963907209)
completed all 840 native trials (840,000 measured samples), all 350 syscall-trace
trials, and 124 of 128 profile trials. Native timings and traces are complete;
the overall CI campaign failed. The portable QUIC four-service short DHAT client
reported `remote exception: connection stopped`. Its long trial and both frozen
legacy DHAT trials for that workload were not run. All Callgrind trials completed.
The logs do not establish the disconnect's cause. A ten-second QUIC idle timeout
exists and instrumentation slows service-chain startup, but that remains a
hypothesis, not a diagnosed timeout. All five native repetitions of this portable
case completed. Both droplets were destroyed and the cleanup step passed.

The raw artifact is `dedicated-benchmark-report`, ID `12509424287`, 266,010,330
bytes, SHA-256 `3ea17984ee85da3e66b99d707fdb79af0c61960eb9ed228f864c289b0411bd5a`.
The downloaded archive matches that digest. See [initial-summary.json](initial-summary.json)
for executable hashes, source, hardware, native samples summarized with spread,
profiles, host counters and trace counts. [The full first-run table](initial-tables.md)
includes every latency cell, all supported allocation comparisons, successful
socket calls, all attempts and complete 100-operation capture totals.

Relative to frozen legacy, geometric mean latency changed by **+0.36% TLS** and
**+1.14% QUIC**; only 11/24 TLS and 10/24 QUIC means were lower. TLS concurrent-32,
deep-32-concurrent-32 and four-service/eight-client means were respectively 1.32%,
1.58% and 1.66% lower. QUIC dependent reads regressed 18.72%, with substantial
between-repetition spread (115.494 ± 26.271 microseconds versus 97.282 ± 1.263).
The candidate's five QUIC dependent-read means are 98.934, 99.075, 99.704,
159.605 and 120.150 microseconds; the frozen control's are 96.784, 97.601, 96.537,
96.155 and 99.334. Do not erase that regression by substituting medians or
reporting only the favorable cells. The measurements establish the observed
regression, not its causal mechanism. No CPU throttling was recorded; VM steal counters and all host
samples remain in the saved evidence.

Candidate legacy had lower mean latency than C++ in all 24 supported TLS cases,
with a 36.23% lower geometric mean. It was lower than current portable in 23/24
TLS and 21/24 QUIC cells. These results precede the duplicate-wake publication
fix, and are not a claim that the changes made legacy uniformly faster. C++ has
no equivalent QUIC implementation in this benchmark.

| TLS workload | Frozen allocations | Candidate allocations | Reduction |
| --- | ---: | ---: | ---: |
| Dependent read | 120 | 118 | 1.67% |
| Pipelined read | 137 | 135 | 1.46% |
| 32 concurrent calls | 786 | 754 | 4.07% |
| Depth 32, 32 concurrent calls | 802 | 770 | 3.99% |
| Four services, eight clients | 4756.51 | 4564 | 4.05% |

Allocation stacks attribute the four-service reduction to **128 boxed guard
pairs and 64 notification nodes per batch**, with a small residual from the
short/long subtraction. The shallow concurrent case removes 32 guard boxes,
all on the service; its client allocation count remains 394. Notification reuse
has no steady allocation reduction in that synchronous fixture because calls
finish before entering the pending-task queue. In the service chain, cached
notifications at the first service remove 64 allocations while the other hops
still use the larger queue. [Allocation type counts](initial-allocation-types.json)
retain per-peer attribution. The `ReturnGuard` matching category includes the
old boxed pair containing a ReturnGuard; it is not a count solely of Rc owners.

The saved Callgrind profiles show only 0.13–0.58% fewer instructions in these TLS
cases. Allocator routines account for 18.95% of shallow concurrent instructions,
5.45% of deep concurrent instructions and 14.98% of the four-service case.
Deep concurrent serialization accounts for 30.60%, TLS crypto 18.28% and explicit
memory routines 10.50%. These are exclusive simulated instruction categories,
not hardware cycle shares; inlined work can be attributed to Rust library paths.
See [cost categories](initial-cost-categories.json).
Matching mutex/futex source frames accounts for about 754 simulated instructions
in concurrent-32 and 6,694 in the four-service operation (0.10% and 0.07% of their
respective totals). [The matched functions](initial-locking-functions.json)
include local output metrics, segment-cache allocation/release and shared-future
polling. This is source attribution, not an upper bound on synchronization cost:
inlining can place atomic work elsewhere, and these counts do not measure lock
waits or cache-line stalls. They do not establish a contended-lock bottleneck.

## Remaining architecture gaps

1. Completion state is still separately allocated beside existing ownership.
   The four-service TLS operation has 448 `completion::Slot` allocations and
   128 `QuestionRef` allocations. The 32-call client alone has 32 question reply
   channels beside 32 question owners. Embedding reply storage in the owner is
   the next concrete fusion candidate, but must preserve pipeline-only interest,
   cancellation, disconnect, reentrant wake callbacks and result destruction.
2. Protected asynchronous dispatch still forks a shared future between the
   executor and caller. The same service-chain profile records 384 allocations
   of shared-future states/wrappers across forwarding calls. Simply putting the
   task behind another completion channel adds a scheduling hop and changes who
   drives completion; replacing it needs a direct completion design with the
   existing protected-method lifetime contract.
3. More than eight pending tasks promote permanently to `FuturesUnordered`.
   Reuse is bounded to the small path. The four-service profile has about 161
   unordered queue-node allocations, of which 33 belong to the unchanged
   client-side `try_join_all` fixture. Pooling driver nodes requires explicit stale-waker
   and storage-reuse rules; it is not evidence of lock contention.
4. Strict read/write parity with C++ remains unmet. TLS fanout uses 14 positive
   reads and 2 writes per operation versus C++'s 10/10. Concurrent-32 uses 4/2
   versus 2/2. Dependent and pipelined reads match at 4/4 and 2/2. The current
   allocation changes do not change TLS framing or remove these extra reads.
5. The profiler must finish remaining controls after one fixture fails while
   still returning a failed campaign. That orchestration change is included in
   the final candidate. Analysis exposes failures and unmatched profile pairs;
   it never computes a marginal count from a missing short or long trial.
