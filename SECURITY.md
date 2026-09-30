# Security policy

## Report a vulnerability

Email [huynhderic@gmail.com](mailto:huynhderic@gmail.com) with the subject
`Capn't Proto security report`. Do not disclose suspected vulnerabilities in public
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

For a problem in vendored code that affects Capn't Proto, report it here with the
component and revision. The maintainer can coordinate with the upstream project
while accounting for Capn't Proto's fork-specific behavior.

## Deployment and support boundary

This is a 0.x developer preview. The custom Noise/quiche binding has not received
an independent production cryptographic review. Supported deployment is explicitly
provisioned peers with pinned identities, configured resource limits and trusted
local storage. Treat introduction tickets and SturdyRefs as secrets.

The handshake is `Noise_IK_25519_ChaChaPoly_BLAKE3`; introduced sessions use
IKpsk2 with the same primitives. Application 0-RTT is rejected. This transport
does not interoperate with TLS QUIC. Checksums protect storage against accidental
corruption, not a malicious writer or rollback. Runtime capability hooks are
never persisted as authority.

Correctness bugs in advertised behavior block a release. Optional features and
unbounded proof goals do not. See [release acceptance](docs/RELEASE_ACCEPTANCE.md) for current
implementation evidence and remaining production qualification.

## Maintainer setup

Publishing this file makes the reporting policy available; it does not enable
GitHub's private reporting form. Follow the
[repository setup guide](docs/GITHUB_SETUP.md) to enable that feature and its
notifications after the repository is uploaded. GitHub documents the separate
[private vulnerability reporting setting](https://docs.github.com/en/code-security/how-tos/report-and-fix-vulnerabilities/configure-vulnerability-reporting/configure-for-a-repository).
