# Security policy

## Report a vulnerability

Email [huynhderic@gmail.com](mailto:huynhderic@gmail.com) with the subject
`Capntproto security report`. Do not disclose suspected vulnerabilities in public
issues, pull requests, or discussion threads.

If the GitHub repository offers **Security → Advisories → Report a vulnerability**,
you may use that private form instead. The email address remains available if
private vulnerability reporting is not enabled or you are using a source bundle.

Include the affected commit or source manifest hash, operating system,
architecture, toolchain, enabled features, expected and observed behavior, and a
minimal reproduction. Explain the suspected impact and any prerequisites for an
attacker. Use synthetic data and disposable test identities; exclude live
credentials, private peer keys, introduction tickets, SturdyRefs, and personal data.

The maintainer will assess the report, request additional details if needed, and
coordinate a fix and disclosure where applicable. Response timing depends on
maintainer availability; no response deadline or bug bounty is promised. If you
have not received a response, follow up on the same private email thread. Please
coordinate public disclosure and any reporter credit with the maintainer.

## Supported versions

The current development version on the default branch is the security maintenance
target. Reports about older preview snapshots are welcome, but a fix may require
updating to the current source. There is no stable release or long-term support
branch at this stage, and no promise of backports to earlier snapshots.

For a problem in vendored code that affects Capntproto, report it here with the
component and revision. The maintainer can coordinate with the upstream project
while accounting for Capntproto's fork-specific behavior.

## Deployment and support boundary

This is a 0.x developer preview. Native identity, admission and capability
integration still require independent production security review. Provision peers
with pinned Ed25519 identities, configured resource limits and trusted local
storage. Treat introduction tickets and SturdyRefs as secrets.

`rpc::tcp` is plaintext and unauthenticated. Use `rpc::tls` or native TCP
for TLS 1.3. Quiche uses standard QUIC v1/v2 with TLS 1.3.
Native sessions require mutual peer-key authentication and reject application
0-RTT. Reservation secrets authenticate admission; they do not independently
contribute to TLS traffic encryption. Conventional CA/name-validated TLS and
mTLS remain available through the two-party adapters.
Checksums protect storage against accidental corruption, not a malicious writer
or rollback. Runtime capability hooks are never persisted as authority.

Correctness bugs in advertised behavior block a release. Optional features and
unbounded proof goals do not. See [release acceptance](docs/wiki/Release-Acceptance.md) for current
implementation evidence and remaining production qualification.

## Maintainer setup

Publishing this file makes the reporting policy available; it does not enable
GitHub's private reporting form. Follow the
[repository setup guide](docs/wiki/GitHub-Setup.md) to enable that feature and its
notifications after the repository is uploaded. GitHub documents the separate
[private vulnerability reporting setting](https://docs.github.com/en/code-security/how-tos/report-and-fix-vulnerabilities/configure-vulnerability-reporting/configure-for-a-repository).

## Code scanning and diagnostics

GitHub CodeQL default setup uses the extended security suite for Actions, C/C++,
Python and Rust. The [review record](.github/codeql-review.json) documents the
initial alert triage without suppressing queries. Maintain a
single setup, including the vendored runtime code; do not add a second CodeQL
workflow or blanket-exclude dependencies or security queries. Findings require
source/data-flow review. Dismiss only confirmed false positives or test fixtures,
with the precise reason recorded on the alert; revisit that conclusion when the
relevant code or scanner model changes. Protocol-defined public salts must remain
interoperable; initialized buffers filled by a checked CSPRNG/HKDF are not hard-coded
traffic keys. Memory-safety findings require checking null guards, wire bounds and
ownership invariants, alongside the Miri qualification suite.

Ordinary QUIC debug formatting redacts address-validation tokens, stateless-reset
tokens and unknown transport-parameter payloads. It retains public packet/stream
IDs, lengths and flow-control counters. Explicit key logging and optional qlog
traces can contain sensitive material: enable them only for controlled diagnostics,
restrict access and retention, and never include them in public reports. The native
backend does not enable qlog by default.
