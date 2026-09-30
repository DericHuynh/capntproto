#!/usr/bin/env bash
# cargo bench omits --crate-type for executable targets. cargo-auditable 0.7.6
# only embeds data when bin/cdylib is explicit. RUSTC_WRAPPER runs outside its
# RUSTC_WORKSPACE_WRAPPER, so make rustc's default explicit before it inspects
# the arguments. Library dependencies retain their original crate types.
set -euo pipefail
compiler=$1
shift
has_name=false
has_type=false
for arg in "$@"; do
    case "$arg" in
        --crate-name|--crate-name=*) has_name=true ;;
        --crate-type|--crate-type=*) has_type=true ;;
    esac
done
if "$has_name" && ! "$has_type"; then
    set -- "$@" --crate-type bin
fi
exec "$compiler" "$@"
