# Portable receive batching and concurrent completion — 24 October 2026

Portable/legacy latency parity remains open. This follow-up keeps a bounded 32-frame receive batch. It rejects a lazy maintenance selector after profiling showed more instructions and indirect branch misses, and rejects selective completion scans after the service-chain profile showed no instruction reduction. All timing numbers below are exploratory; the dedicated latency campaign is recorded separately.

## Changes and measurements

Every experiment was committed and built before its measurements. Native timing ran separately from compilation and instrumentation. Instruction and branch-miss counts are Callgrind simulations summed across all peers with matched startup subtraction; they are not hardware performance counters. Step 7 and the initial step-8 comparison cover 14 case/transport rows. Step 9 covers all 48; after rejecting it, the retained step-8 binary receives a separate full 48-row comparison. Native controls freeze both drivers at `b311711`; current legacy and pinned C++ are also interleaved. The extra four-service profile freezes portable at step 8 to isolate the completion change.

| Step | Decision | Unary instructions | Pipeline instructions | Concurrent-32 instructions |
| --- | --- | ---: | ---: | ---: |
| 7 | Reject lazy readiness selection; reverted | 47,196 | 146,066 | 868,148 |
| 8 | Keep 32-frame input batch | 46,735 | 145,041 | 855,928 |
| 9 | Reject selective scans after sibling replies; reverted | 46,781 | 145,163 | 855,833 |

The lazy selector tested classes one at a time instead of rebuilding a readiness bitmask. That reduced source-level checks but added indirect control flow: concurrent indirect misses rose from 672 to 1,450, and instructions increased across all three profiled core cases. Commit `f643f4f` reverts the experiment. The existing bitmask stays.

The 32-frame batch follows legacy's receive quantum while honoring smaller caller budgets. It preserves shutdown, capacity-pressure and output-progress checks. Concurrent-32 simulated instructions fell 0.81% against its frozen control. Unary and pipeline instruction counts were effectively unchanged. The 140-frame control-flood regression still requires output progress within the original 32-frame bound.

The rejected completion experiment addressed ordinary service completion, which restarts call and foreign-work scans whenever any sibling worker remains alive. The experimental path kept worker/maintenance readiness and retained all checks for queued calls, foreign work, commands, hook inbox changes and notifications. It added no retained readiness state and changed no public API. The extended parked-sibling regression failed on step 8 with both unrelated lanes active, then passed on step 9 across the existing budget/output scenarios. Despite that targeted reduction in lane activations, the benchmark did not show lower instruction cost; commit `ecdead5` reverts the experiment and its test extension.

| Rejected step-9 experiment: four-service, depth-16, concurrent-32 | Instructions per whole batch | Conditional misses | Indirect misses |
| --- | ---: | ---: | ---: |
| legacy | 8,938,070 | 17,786 | 10,709 |
| portable | 9,864,455 | 30,097 | 11,447 |
| portable-baseline | 9,857,171 | 27,477 | 12,175 |
| cpp | 17,434,161 | 74,516 | 86,935 |

This service profile sums the client and all four services, subtracting matched 5-operation from 15-operation captures with two warmups. Step 9 changes instructions by +0.07% against step 8. Conditional misses rose while indirect misses fell. This provides no clear performance benefit for the additional branching, so the experiment was rejected. These simulated counters measure work, not native time.

## Remaining gaps

The [full timing table](comparison.md) reports mean ± sample SD of five fresh-process run means. Portable is lower than frozen portable in 25/48 rows, lower than legacy in 8/48, and lower than C++ in 23/24 supported TLS rows. Its geometric-mean changes are -1.93% versus frozen portable, +14.00% versus legacy, and -31.56% versus C++. These point estimates do not close the performance target.

Pipeline remains a CPU gap: 145,041 instructions versus legacy's 95,832 (+51.4%). The [function counts](function-counts.json) retain readiness probes, hook collections and coordinator visits. Reducing repeated orchestration across capability-pipeline transitions remains more promising than reducing common-path socket calls; capability and bounds validation must remain correct.

The [socket table](io-comparison.md) uses five paired (100−10)/90-operation slopes across every peer. Common dependent, pipeline, fanout and concurrent paths retain their existing low call counts. The four-service/eight-client case remains a separate coalescing gap; its complete mean/spread comparison is included. Bulk read counts are variable and must be compared with their spread. [Attempted calls](io-attempt-comparison.json) retain EAGAIN/unsuccessful attempts separately from successful ciphertext socket calls.

The [allocation table](allocation-comparison.md) covers unary, pipeline and concurrent-32 on TLS/QUIC. It is a startup-subtracted regression diagnostic, not a repeated full-catalog allocation qualification. C++ has no QUIC fixture.

The [host captures](host-health.json) show a four-CPU outer quota; native throttled periods increased by 0. Maximum per-trial steal was 11.8% aggregate and 50.0% on a selected CPU. Droplet measurements remain the basis for latency qualification.

## Architectural assessment

There is no demonstrated hard limit preventing portable from matching legacy. There is a structural cost in how protocol progress crosses the hosted execution, hook bridge, portable driver and storage boundaries. The retained candidate's Callgrind pipeline cost is 145,041 instructions versus 95,832 for legacy (+51.4%); concurrent-32 is 855,928 versus 674,518 (+26.9%), while unary is much closer (46,735 versus 45,984, +1.6%). These are simulated instruction counts, not latency ratios. The common pipeline already matches legacy's framing and successful socket counts, so the large pipeline gap is above the physical transport.

1. **Nested scheduling with coarse invalidation.** The coordinator selects application lanes, the driver selects among 16 maintenance classes, and calls/workers each retain their own ready queues. `progressed()` reactivates call, foreign and maintenance lanes; `ready_work()` then reconstructs readiness from producer-owned queues. There are already precise operation wakes and idle certificates, but general progress paths discard part of that precision. [Coordinator work](../../../../crates/capntproto-rpc/src/scoped/wire/coordinator/work.rs#L16) and [driver maintenance](../../../../crates/capntproto-rpc-wire/src/owned/connection/maintenance.rs#L21) show the boundary. The pipeline performs 17 readiness probes, 13 hook collections and 13 work-budget entries per operation. The architecture change would be to carry the affected owners and work kinds through every transition, allowing one connection work loop to consume ready owners under a shared budget. The existing effect-reporting `ControlContext` is a useful starting point, but its general network effect still invalidates broadly. Merely replacing the bitmask with lazy checks was slower; a producer-maintained queue needs its own measurement.

2. **A bridge between two lifetime models.** Hosted generated clients use owned hooks while the portable engine uses scoped graph handles and explicit reclamation. `ClientHooks` translates releases, route changes and metadata notifications between them, and dispatch/completion repeatedly calls `collect_hooks()`. This is not a duplicate protocol ledger, and stable turns already avoid whole-table scans. It is repeated adapter work between ownership models. [Hook synchronization](../../../../crates/capntproto-rpc/src/scoped.rs#L685) and [wire metadata](../../../../crates/capntproto-rpc/src/scoped/wire/resources.rs#L57) identify the remaining bridge. A shared retained endpoint/operation context, with notifications emitted when its owner actually changes, could remove repeated reconciliation while preserving the portable protocol authority.

3. **Storage and publication contracts force representation work.** The hosted adapter copies buffered frames into portable message storage below the shared-frame threshold. Prepared output is validated and either copied into leased storage or retained through the supported shared-parts path. These contracts support fixed caller-backed storage, growing pools and hosted owned transports together; equal allocation counts do not remove copying, ownership checks or publication bookkeeping. [Receive adaptation](../../../../crates/capntproto-rpc/src/scoped/wire/coordinator/transport/vat.rs#L282) and [prepared publication](../../../../crates/capntproto-rpc-wire/src/owned/connection/prepared_return.rs#L389) show the work. The next design should carry an immutable owned message and its validated metadata through the hosted path, specializing storage policy without duplicating the protocol implementation. Validation proofs must remain tied to the actual bytes, capability table and reader limits. Sharing every tiny frame is not automatically faster than copying it.

4. **Service-chain batching still needs investigation.** The four-service/eight-client trace uses about 32.6 reads and 32.8 writes per operation for portable, versus 27.3/27.3 for legacy. The hosted publication sink already transfers directly into the physical outgoing queue; there is not a second portable output FIFO to delete. The remaining question is when ready completions reach the flush boundary across connections. [The pump](../../../../crates/capntproto-rpc/src/scoped/wire/coordinator/pump.rs#L120) is the relevant boundary. Trace ready-owner execution and flush decisions before changing this policy; the extra calls establish a coalescing gap, not its complete cause.

Legacy's [receive loop](../../../../crates/capntproto-rpc/src/rpc.rs#L1265) hands an owned message directly to [dispatch](../../../../crates/capntproto-rpc/src/rpc/dispatch.rs#L506). C++ uses the same broad receive-and-dispatch shape in `messageLoop()`, with promise machinery handling deferred work. Portable's successful plain-message shortcuts already approach that shape; capability pipelines cross more of the general machinery.

The first redesign should tighten the transition/effect API, then consolidate hosted message ownership and retained validation metadata. Low-level coordinator, driver and storage APIs may need breaking changes to encode those contracts; the generated RPC interface need not inherently change. Adding more special-case scheduling flags, replacing Tokio, or changing TLS again is not supported as the next step by these traces. No proposed architectural replacement has yet demonstrated all-case legacy parity.

## Validation and CI

Both steps 8 and 9 passed 375 RPC, 163 wire, 30 secure TLS/QUIC and 32 serialized Unix tests: 600 test executions per step, plus warnings-denied RPC Clippy and formatting. The rejected completion regression was first run against step 8 and failed as expected; its log is retained under steps/9/validation. The final tracked source tree is identical to the tested step-8 tree (`5a7a101d`); no extra build or repeated test run is claimed for the revert. Full workspace coverage was not rerun. The rejected step 7 initially hit an OOM during an RPC build, then passed the targeted checks after archived build/evidence storage was reduced; the failed build is preserved.

The [new dedicated run](https://github.com/DericHuynh/capntproto/actions/runs/40116177710) uses `run-latency`, all 48 TLS/QUIC rows, five repetitions and a frozen `b311711` portable control alongside current legacy and C++. Status when recorded: release-bundle build in progress; dedicated measurement has not started. It uses the shorter CI path introduced in the [preceding report](../2026-10-24-portable-latency/README.md). The existing full campaign [40108765911](https://github.com/DericHuynh/capntproto/actions/runs/40108765911) is preserved; its shared measurement slot can delay the new run. No run was canceled and no main-branch merge was performed.

## Evidence

[Provenance](provenance.json) records the runtime, binary and fixture hashes, all three experiments, the frozen controls and the verified raw archive. The archive preserves every trial, native sample, all-peer profile, socket trace, validation log and host capture for steps 7–9, including both the selected and full step-8 comparisons. The rejected step-9 full timing table is retained under steps/9. It remains workspace evidence; the dedicated workflow publishes its own artifacts.
