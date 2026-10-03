---
name: Bug report
about: Report a reproducible failure or incorrect behavior in Capntproto.
title: "[Bug]"
labels: bug
assignees: ''

---

<!-- For suspected vulnerabilities, email huynhderic@gmail.com privately instead.
Read SECURITY.md before sharing details. Remove credentials, peer private keys,
introduction tickets, SturdyRefs, and personal data from logs and attachments. -->

## What happened?

Describe the observed behavior and its impact.

## What did you expect?

Describe the expected behavior and any documented contract involved.

## Reproduction

Provide the smallest code, schema, input, or sequence that reproduces the problem.
Include exact commands and whether the failure is consistent or intermittent.

## Environment

- Commit or source bundle version:
- Operating system and architecture:
- Rust version (`rustc --version --verbose`):
- Enabled Cargo features and build profile:
- Relevant native tool versions (for example, Cap'n Proto, Clang, Java/TLC):

## Relevant output

Paste the error and relevant redacted logs as text. For a model or simulation
failure, include the configuration, seed, and retained trace/report if available.

## Regression information

If known, identify a working revision, related issue, or workaround. Describe
which checks you ran and their outcomes; reporting a bug does not require running
the complete suite.
