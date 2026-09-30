# Reports

Current quality and benchmark reports are generated under `target/quality/` and
published as GitHub Actions artifacts. See [the quality guide](../../docs/QUALITY.md)
for report generation, coverage gates, and the comparison charts in each report's
README.

The JSON and JSONL files in `storage-benchmark/` and `eae-integration/` are frozen
research measurements included in source distributions. They are not results from
the current CI run.

Historical logs and model-checker output referenced by older design documents
are local archives and are not included in this repository. Model source and
verification fixtures remain under `research/baseline/`, `verification/`, and
`test-support/verification/`.
