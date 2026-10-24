# Follow-up: upstream rustls suite validation

The initial optimization report included repository differential/integration tests,
but not rustls's own suite. This follow-up closes that specific validation gap
for the upstream crate on Linux x86_64; it does not claim the full upstream CI matrix.

The exact upstream workspace at `f04804fa13341a9b1dc02b5d1dd8a746f2c4b7fb` was
restored, including the fixtures omitted from the published crate. `LOCAL.patch`
applied with zero fuzz. All five patched source files were verified byte for byte
against the vendored files. Test bodies, fixtures and the upstream lockfile were
unchanged. Implementation commit remains `58205ddc5b3f0841848e836327b6c249d5200777`.

| Feature configuration | Passed | Failed | Upstream ignored | Unbuffered passed |
| --- | ---: | ---: | ---: | ---: |
| ring | 540 | 0 | 8 | 27 |
| aws-lc-default | 546 | 0 | 8 | 27 |
| all-non-fips | 947 | 0 | 8 | 54 |

Counts include unit tests, all declared integration targets and doctests. No
test-name filters were used. Feature runs overlap; these are 2,033 passing test
executions, not 2,033 unique tests. No runtime fixes were required.

Each run retains three upstream-ignored BoGo entry points and five ignored
documentation examples. The separate BoGo protocol suite, FIPS, other workspace
packages, fuzzing, MSRV, and cross-platform/toolchain CI matrix were not run.
The Go compiler needed by BoGo is not installed in the test container.

The all-non-FIPS run selects every feature declared in the upstream manifest
except `default` and `fips`; this includes both providers, TLS 1.2, compression,
custom-provider, hashbrown and read_buf. It checks the default owned-record
fallback as well as the optimized ring path. The ring and AWS-LC runs also
exercise their respective isolated provider configurations.

## Reproduction

From a clean checkout of that upstream commit, apply our patch in its `rustls/`
subdirectory. Confirm the resulting five source files match `vendor/rustls/`, then
run these commands from the upstream workspace root:

```sh
cargo test --locked -p rustls --no-default-features --features ring,std,tls12,logging
cargo test --locked -p rustls
cargo test --locked -p rustls $(python3 admin/all-features-except fips rustls)
```

This run used Rust 1.97.0, `CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_INCREMENTAL=0`,
`CARGO_BUILD_JOBS=1`, and Clang for C/C++. [Source provenance](source.json) records
the archive, patch, lockfile hashes and compiler. [Exact commands](checks.json)
and [per-target results and ignored tests](summary.json) are retained.

Complete unedited output: [ring](ring.log.gz), [AWS-LC](aws-lc-default.log.gz),
[all non-FIPS features](all-non-fips.log.gz). These logs are gzip-compressed.
The original performance evidence archive predates this follow-up; the logs here
are independently committed evidence and are not claimed to be inside it.
