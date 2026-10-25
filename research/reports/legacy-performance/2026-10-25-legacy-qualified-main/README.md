# Qualified changes selected for main

The user requested stopping the optimization campaign, excluding ambiguous experiments, and combining the clear improvements onto main. The retained runtime source is exactly c6900f3210c7d8505d47ea8821e419c2f492b2ab, before every branchless scheduler experiment. Its complete hosted checks,70 selected Miri executions and240 five-repeat instruction/allocation trials passed in [run40205699992](https://github.com/DericHuynh/capntproto/actions/runs/40205699992). Rejected experimental history remains on the qualification branches.

## Retained

- Previously qualified legacy allocation reductions, shared request/status/wake ownership, encrypted framing, direct-output rustls and quiche buffer APIs.
- Clear compiled-work reductions: removing forced request inlining saves8 instructions per request; last-question cleanup removes the duplicate table lookup (lookup self-work falls1344 instructions per32-call batch); response access saves1 instruction per access. Individual reports retain whole-program regressions as well as these component gains.
- Qualified no_std/caller-owned storage interfaces, concrete request/response/future/client owners, borrowed scheduling, flat hosted question records, and task-owned input/result/context guards. These have real fixed-storage tests; they do not imply a complete allocation-free connection driver.
- Movable reading from the existing message pool, production incoming ownership, and immediate buffer reuse on completion, cancellation and malformed framing. Eight generated-dispatch cases add zero blocks/bytes in both fixed and allocator-enabled profiles.
- Corrected native benchmark provenance: record the pinned benchmark manifest's release settings and explicit profile environment values, instead of claiming fat LTO and one codegen unit without configuration evidence.

## Excluded

All branchless scheduler/Return experiments, including the threshold form and single-byte publication state, are absent from the runtime tree. The original branch behavior and scheduler projection fields are retained. The independently qualified branchless Return release added16 instructions for unary and512 for concurrent/deep operations (paired SD0), and was rejected. Threshold whole-program Bcm/Bim results were mixed despite component improvements; they are not promoted as a speedup.

The previously rejected idle-sentinel, maintenance-mask and sibling-idle experiments remain reverted. The temporary nested hosted question record is not retained; the qualified flat projection is used. The newly proposed native checkout/path orchestration change is not promoted without its own completed qualification. The threshold-only dedicated native campaign40214675223 was canceled during compilation, before droplet measurement.

## Validation scope

The runtime crates, verification fixtures, benchmark code, TLS vendor and tooling source match the qualified pre-experiment snapshot. Only the provenance correction and this selection/evidence documentation are added. Local final checks and publication identity are recorded alongside this report. Full no-alloc RpcSystem integration and universal latency/CPP superiority remain open; no such claim accompanies this publication.
