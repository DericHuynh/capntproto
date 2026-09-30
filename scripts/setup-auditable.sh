#!/usr/bin/env bash
# Run from the repository root. Works in bash on Linux, macOS and Windows CI.
set -euo pipefail
repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
install_root="$repo_root/target/auditable-tools"
real_cargo=$(command -v cargo)
if [[ $real_cargo == "$install_root/wrapper/cargo" ]]; then
    IFS= read -r real_cargo < "$install_root/wrapper/real-cargo"
fi
# The auditable bootstrap is the only ordinary cargo install. All subsequent
# Cargo commands, including installation of CI tools, use cargo-auditable.
"$real_cargo" +1.97.0 install cargo-auditable --version 0.7.6 --locked --root "$install_root"
export PATH="$install_root/bin:$PATH"
"$real_cargo" +1.97.0 auditable install rust-audit-info --version 0.5.4 --locked --root "$install_root"
mkdir -p "$install_root/wrapper"
printf '%s\n' "$real_cargo" > "$install_root/wrapper/real-cargo"
cp "$repo_root/scripts/auditable-cargo.sh" "$install_root/wrapper/cargo"
cp "$repo_root/scripts/auditable-bench-rustc.sh" "$install_root/wrapper/auditable-bench-rustc"
chmod +x "$install_root/wrapper/cargo" "$install_root/wrapper/auditable-bench-rustc"
printf "Activate auditable Cargo in this bash session:\nexport PATH=\"%s/wrapper:%s/bin:\$PATH\"\n" "$install_root" "$install_root"
