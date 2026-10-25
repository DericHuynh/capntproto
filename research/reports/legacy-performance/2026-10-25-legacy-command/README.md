# One legacy scheduler command representation

Source `f7918be`; qualification `cb6e67e6b308c3e21fd4e8906f969de0983aa74a`,
[run 40168435467](https://github.com/DericHuynh/capntproto/actions/runs/40168435467), successful.
Control `df796ab1d55fbda57ef39c02c60767c276c32991`. The only implementation change aliases
hosted admission to the engine Command rather than translating a duplicate enum.

The repeated component trace disproves the hoped-for instruction saving: scheduler plus
readiness Ir increases 20/40/20/28/30/28 per operation across the six cases, with zero
sample spread. Bc is unchanged. Callback Bi decreases two; all other Bi counts are equal.
The earlier scheduler extraction's additional conditional branches remain unresolved.
Do not infer an instruction improvement from deleting source code or fewer indirect calls.

Whole-binary simulated Bim improves in all six cases (2.254–37.276%), but this includes
linked layout and predictor aliasing. Concurrent Bcm worsens 1.300%. Ir increases
0.007–0.055% overall. This is not native latency or hardware branch-miss evidence.
TLS allocations remain 7/58/202 blocks in unary/pipelined/concurrent. Both transports'
bytes, spreads and paired samples remain in comparison.json; QUIC remains noisy.

Profiles repeat five times, alternate controls, and retain all 240 trials. Selected
self-work attribution reconciles the total per-metric deltas. Assembly was streamed from
the archive and restricted to scheduler and direct owner/hook functions for analysis;
the assembly, symbols and raw Callgrind/DHAT files remain in the CI artifact. Run analyze.py against the artifact
extraction with its original symbol files to reproduce attribution and layout analysis.

Artifact 12589822995: 84,968,850 bytes; SHA-256
`d48eea2057ac82921837736c149d2f2f4420c2e9ff1fa147625c451a58393e1f`.
