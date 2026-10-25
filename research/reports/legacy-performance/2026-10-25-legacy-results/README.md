# Caller-owned fallible legacy results

Source `f516105`; control `f7918be`; qualification
`07ca061ecef9dad5ae006852d59e6a77f70c93fa`,
[run 40169817346](https://github.com/DericHuynh/capntproto/actions/runs/40169817346), successful.

Results<T,H>/ResultStorage<T> accept fixed roots, selected Return content fields,
borrowed providers and RefMut guards. Fallible editors preserve unknown fields and
existing data on rejected growth; they never use heap fallback. Params/Response
can expose the same generated FixedSchema views. Hosted boxed constructors and
ordinary get/init methods retain their field and dispatch representation.

1,129 unit/integration executions and 47 documentation tests pass (8 documentation
examples ignored), including 109 no-default wire, 378 hosted RPC and 30 encrypted
RPC cases. Six result-owner tests check zero allocations and bytes, fixed-root
capacity rejection, envelope-header and existing-data retention, unselected-union
rejection, exclusive pooled guards and legacy Owned-to-FixedSchema selection.
Strict-provenance Miri passes these six cases on both widths. The workflow runs
default and explicit no-default twice, but wire's defaults are empty: this is
24 executions of 12 distinct test/width combinations, not an allocating Miri profile.
A follow-up qualification adds the explicit alloc feature.

The same fixed-Return probe executes on the host and links for M4 with no allocator
or undefined symbols. It is not a complete legacy RPC conversation or board execution;
embedded-link.json retains the exact source/ELF checksums and scope.

Across five repetitions, selected owner and scheduler/owner self Ir/Bc/Bi are
exactly equal in all six cases. All 139 common normalized assembly functions are
structurally identical; this normalizes linked addresses and is not byte equivalence.
The selector includes scheduler and direct Params/Response/hook functions. It does
not cover every ResultsHook implementation, and no claim extends beyond that scope.
TLS blocks remain 7/58/202. Both transports' bytes, spreads and all paired samples
remain in comparison.json. The deep whole-case +256.355 Ir delta includes +254.980
in libc memcpy; it is retained rather than discarded. Simulated branch misses are
essentially stable here, and native latency remains unqualified.

240 interleaved trials use 10/50-workload slopes. Attribution reconciles all total
metric deltas. Artifact 12591055132 is 81,246,097 bytes, SHA-256
`c33697123caeff17ca9326a8657369ac9e5cd2ee0527be55a286bac7e260c719`.
Raw profiles, symbols and assembly remain in that artifact; analyze.py can reproduce
the retained attribution using the selected extraction and original symbol files.
