#!/usr/bin/env bash
# Installed as `cargo` by setup-auditable.sh, following cargo-auditable's
# documented PATH replacement. Keep rustup selection before the subcommand.
set -euo pipefail
shim_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
IFS= read -r real_cargo < "$shim_dir/real-cargo"
toolchain=''
if [[ ${1:-} == +* ]]; then
    toolchain=$1
    shift
fi
if [[ ${1:-} == auditable ]]; then
    shift
fi
if [[ ${1:-} == bench ]]; then
    if [[ -n ${RUSTC_WRAPPER:-} && $RUSTC_WRAPPER != "$shim_dir/auditable-bench-rustc" ]]; then
        echo 'Auditable benchmarks require their crate-type wrapper; RUSTC_WRAPPER is already set.' >&2
        exit 1
    fi
    export RUSTC_WRAPPER="$shim_dir/auditable-bench-rustc"
fi
# Bash 3.2 (the macOS runner default) treats empty arrays as unset under -u.
if [[ -n $toolchain ]]; then
    exec "$real_cargo" "$toolchain" auditable "$@"
fi
exec "$real_cargo" auditable "$@"
