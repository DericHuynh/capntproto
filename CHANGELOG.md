# 0.1.0 developer preview (unpublished)

- Capability RPC runtime and field-oriented Rust generator with bounded TLC,
  Rust trace replay and selected C++ reference/interoperability checks.
- Experimental Noise IK/IKpsk2 25519/ChaChaPoly/BLAKE3 transport, dynamic schemas,
  bulk/realtime services and explicit three-party introductions.
- Typed durable objects, publication history, atomic batches and revocable realms.
- Storage format **RPROTO04** independently checks record framing before tail
  recovery and stabilizes the recovered file and directory before serving.
  Versions 1–3 are rejected; there is no automatic migration.
- Coordinated core/RPC/generator/futures dependencies work from an external
  application without root Cargo patches. Distribution is a source bundle.
- EAE comparison and private integration probe; the production backend remains
  the whole-entry store. See `docs/EAE_BENCHMARKS.md` for measured tradeoffs.

This is an experimental preview, not complete C++ API parity or a production
security qualification. See `docs/RELEASE_ACCEPTANCE.md` for outstanding gates.
