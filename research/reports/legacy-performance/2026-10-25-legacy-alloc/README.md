# Actual legacy RPC without std (still allocating)

The alloc feature now compiles legacy RpcSystem, its connection/message/client hooks,
generated owned services, and the optional portable runtime without std. OS adapters,
Unix descriptors, implicit TLS local-client identity, membranes, segment caches and the
default clock remain std integrations. No selection of the bounded driver is involved.

The explicit ClientRegistry holds weak per-interface registrations and is retained by
its client states. All local views of a server must share that registry. This keeps
identity reuse and revocation conflict checks without requiring a global or thread local.
The hosted registry, collection and mutex types remain canonical. The allocator-only
profile uses hashbrown and futures' existing spin support; scoped wake mailboxes use
the same spin backend and release it before invoking a Waker. Platforms require
pointer-width atomics. Segment caching is absent, including its portable public options.

Local checks: eight isolated no_std service/conversation tests pass, covering unary,
retained responses, depth-32 dependent access and pipelining, 32 simultaneous requests,
reverse callbacks, early cancellation and continued connection use, identity deduplication
and revocable alias rejection. Expanded tests only poll after a wake notification;
three-byte framed reads with Interrupted/Pending exercise depth-32 pipelines and
32 concurrent calls, callbacks, retained data, cancellation, truncated EOF and an
oversized header. RPC normalizes framing failure to Disconnected; the fixture
separately records and checks the precise transport cause. The same fixture checks
on thumbv7em-none-eabi. An earlier hosted
all-target Clippy pass and final alloc-only library/fixture Clippy pass with warnings denied;
core-only M4 checks and code-generator unit tests pass. This is not board execution,
an allocation-free link, encrypted transport testing, or native latency measurement.
The final hosted all-target Clippy retry was killed by workspace memory exhaustion
(cgroup OOM kill rose 14 to 15), so CI must recheck the final revision.
Run 40171165717 already passed hosted verification and two-width Miri of the initial
five-case fixture; its repeated hosted profile is pending. The expanded eight-case
wake/framing suite is qualified separately without another runtime performance run,
since only verification code changes.

The remaining no-alloc blockers are real ownership interfaces: Box<dyn ClientHook> and
consuming boxed request/results hooks, Rc-backed connection/question/pipeline owners,
Vec capability/message/export storage and heap-owned erased futures. The read/output,
admission, task and pending providers are available, but those providers must be wired
into these actual protocol owners before the no-alloc target is complete. A core-only
facade check or bounded-driver conversation is insufficient evidence of completion.

## Qualified hosted comparison

Source `8c6c898`, control `f516105`, qualification `1dda852`,
[run 40171165717](https://github.com/DericHuynh/capntproto/actions/runs/40171165717), successful.
Final hosted tests/Clippy, core-only and allocating M4 compilation, and initial
five-case no_std Miri pass. The expanded wake/framing eight-case fixture and
explicit alloc fixed-result owners also passed strict-provenance Miri on both
widths in [run 40172257171](https://github.com/DericHuynh/capntproto/actions/runs/40172257171)
(16 conversation and 12 result-owner executions). It is still allocating RPC.

The 240 repeated work trials retain five samples and all paired spreads.
Selected read-owner and scheduler self Ir/Bc/Bi are exactly equal; 139 common
normalized assembly functions match. This is not every legacy function or byte
identity. TLS blocks remain 7/58/202. All bytes and noisy QUIC slopes are retained.
Whole Ir changes -0.041%..+0.007%, but simulated Bim increases 33.484% for concurrent,
6.148% dependent, 4.280% pipeline and 7.048% deep; callback improves 7.307%.
The 139 identical normalized bodies and their changed linked identities help
investigate predictor/layout effects; they do not establish that every miss is
layout alone or prove no native regression. Native latency remains unqualified.

Artifact 12590408928: 81,362,330 bytes; SHA-256
`78edbe826d98567dbd2d29c8f7422883eb886cfef306c27b50ad7031f869fcab`.
Original assembly/symbols/raw profiles remain in that verified archive.
