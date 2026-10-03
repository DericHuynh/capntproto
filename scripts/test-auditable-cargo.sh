#!/usr/bin/env bash
# Exercise the actual shim with a fake Cargo, including on macOS Bash 3.2.
set -euo pipefail
repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
test_root=$(mktemp -d)
trap 'rm -rf -- "$test_root"' EXIT
shim_dir="$test_root/path with spaces"
mkdir -p "$shim_dir"
cp "$repo_root/scripts/auditable-cargo.sh" "$shim_dir/cargo"
cat > "$shim_dir/fake-cargo" <<'SH'
#!/usr/bin/env bash
printf '%s\0' "$@"
printf '%s\0' "${RUSTC_WRAPPER:-}"
exit "${FAKE_CARGO_EXIT:-0}"
SH
chmod +x "$shim_dir/fake-cargo"
printf '%s\n' "$shim_dir/fake-cargo" > "$shim_dir/real-cargo"
unset RUSTC_WRAPPER FAKE_CARGO_EXIT

check() {
    local expected_wrapper=$1
    shift
    printf '%s\0' "$@" > "$test_root/expected"
    printf '%s\0' "$expected_wrapper" >> "$test_root/expected"
    cmp "$test_root/expected" "$test_root/actual"
}

# Empty toolchain, spaces, empty arguments and shell metacharacters survive.
bash "$shim_dir/cargo" build --target-dir 'space dir' '' "literal \$value" > "$test_root/actual"
check '' auditable build --target-dir 'space dir' '' "literal \$value"
bash "$shim_dir/cargo" +nightly test --locked > "$test_root/actual"
check '' +nightly auditable test --locked
bash "$shim_dir/cargo" +nightly auditable build > "$test_root/actual"
check '' +nightly auditable build
bash "$shim_dir/cargo" auditable build > "$test_root/actual"
check '' auditable build

# Benchmarks get their required adapter, and never replace a caller's wrapper.
bash "$shim_dir/cargo" bench --no-run > "$test_root/actual"
check "$shim_dir/auditable-bench-rustc" auditable bench --no-run
RUSTC_WRAPPER="$shim_dir/auditable-bench-rustc" bash "$shim_dir/cargo" +nightly bench > "$test_root/actual"
check "$shim_dir/auditable-bench-rustc" +nightly auditable bench
if RUSTC_WRAPPER=other bash "$shim_dir/cargo" bench > "$test_root/actual" 2> "$test_root/error"; then
    echo 'Conflicting benchmark wrapper was accepted.' >&2
    exit 1
fi
test ! -s "$test_root/actual"

status=0
FAKE_CARGO_EXIT=42 bash "$shim_dir/cargo" check > "$test_root/actual" || status=$?
test "$status" -eq 42
echo 'Auditable Cargo argument forwarding passed.'
