#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

make_stub_bin_dir() {
  local dir="$1"
  mkdir -p "$dir"
}

write_common_stubs() {
  local dir="$1"
  local rust_version="$2"
  local cargo_version="$3"

  cat >"$dir/rustc" <<EOF
#!/usr/bin/env bash
echo "rustc $rust_version (stub)"
EOF
  chmod +x "$dir/rustc"

  cat >"$dir/cargo" <<EOF
#!/usr/bin/env bash
if [ "\${1:-}" = "--version" ]; then
  echo "cargo $cargo_version (stub)"
  exit 0
fi
exit 0
EOF
  chmod +x "$dir/cargo"
}

write_stellar_stub() {
  local dir="$1"
  local cli_version="$2"

  cat >"$dir/stellar" <<EOF
#!/usr/bin/env bash
if [ "\${1:-}" = "--version" ]; then
  echo "stellar $cli_version (stub)"
  exit 0
fi
exit 0
EOF
  chmod +x "$dir/stellar"
}

assert_contains() {
  local file="$1"
  local needle="$2"
  grep -F -- "$needle" "$file" >/dev/null 2>&1 || {
    echo "expected '$needle' in $file" >&2
    cat "$file" >&2
    exit 1
  }
}

assert_not_contains() {
  local file="$1"
  local needle="$2"
  if grep -F -- "$needle" "$file" >/dev/null 2>&1; then
    echo "unexpected '$needle' in $file" >&2
    cat "$file" >&2
    exit 1
  fi
}

run_help_case() {
  local output="$TMP_DIR/help.out"
  bash "$ROOT_DIR/deploy.sh" --help >"$output" 2>&1
  assert_contains "$output" "Build, optimize, deploy and initialize the LedgerLens score contract."
  assert_contains "$output" "Usage:"
  assert_contains "$output" "./deploy.sh [options] <network> <admin-identity> <service-address>"
  assert_contains "$output" "--dry-run"
  assert_contains "$output" "--check-toolchain"
  assert_contains "$output" "--manifest <path>"
  assert_contains "$output" "--help             Show this help message."
  # The usage block is extracted from between explicit markers, so shell
  # implementation lines must never leak into help output and the markers
  # themselves must not be printed.
  assert_not_contains "$output" "set -euo pipefail"
  assert_not_contains "$output" "# --- usage ---"
  assert_not_contains "$output" "# --- end usage ---"
}

run_help_marker_integrity_case() {
  # A header edit that removes a usage marker must fail loudly instead of
  # silently truncating --help or leaking the rest of the script.
  local broken_dir="$TMP_DIR/broken-help"
  local output="$TMP_DIR/help-broken.out"
  mkdir -p "$broken_dir/deploy"
  cp -r "$ROOT_DIR/deploy/." "$broken_dir/deploy/"
  sed '/^# --- end usage ---$/d' "$ROOT_DIR/deploy.sh" >"$broken_dir/deploy.sh"

  if bash "$broken_dir/deploy.sh" --help >"$output" 2>&1; then
    echo "expected deploy.sh without the '# --- end usage ---' marker to fail" >&2
    exit 1
  fi

  assert_contains "$output" "missing the '# --- end usage ---' marker"
  assert_not_contains "$output" "set -euo pipefail"
}

run_success_case() {
  local stub_dir="$TMP_DIR/stubs-success"
  local output="$TMP_DIR/success.out"
  make_stub_bin_dir "$stub_dir"
  write_common_stubs "$stub_dir" "1.81.0" "1.81.0"
  write_stellar_stub "$stub_dir" "21.0.0"

  PATH="$stub_dir:$PATH" bash "$ROOT_DIR/deploy.sh" --check-toolchain testnet >"$output" 2>&1
  assert_contains "$output" "Reviewed toolchain OK: rustc 1.81.0, stellar 21.0.0"
  assert_contains "$output" "Manifest validated and toolchain drift check passed."
}

run_rust_drift_case() {
  local stub_dir="$TMP_DIR/stubs-rust-drift"
  local output="$TMP_DIR/rust-drift.out"
  make_stub_bin_dir "$stub_dir"
  write_common_stubs "$stub_dir" "1.82.0" "1.82.0"
  write_stellar_stub "$stub_dir" "21.0.0"

  if PATH="$stub_dir:$PATH" bash "$ROOT_DIR/deploy.sh" --check-toolchain testnet >"$output" 2>&1; then
    echo "expected Rust drift check to fail" >&2
    exit 1
  fi

  assert_contains "$output" "Toolchain drift detected for Rust."
  assert_contains "$output" "Expected: 1.81.0"
  assert_contains "$output" "Actual:   1.82.0"
}

run_cli_drift_case() {
  local stub_dir="$TMP_DIR/stubs-cli-drift"
  local output="$TMP_DIR/cli-drift.out"
  make_stub_bin_dir "$stub_dir"
  write_common_stubs "$stub_dir" "1.81.0" "1.81.0"
  write_stellar_stub "$stub_dir" "22.0.0"

  if PATH="$stub_dir:$PATH" bash "$ROOT_DIR/deploy.sh" --check-toolchain testnet >"$output" 2>&1; then
    echo "expected CLI drift check to fail" >&2
    exit 1
  fi

  assert_contains "$output" "Toolchain drift detected for Stellar CLI."
  assert_contains "$output" "Expected: 21.0.0"
  assert_contains "$output" "Actual:   22.0.0"
}

run_manifest_rejects_unexpected_key_case() {
  local stub_dir="$TMP_DIR/stubs-bad-manifest"
  local bad_manifest="$TMP_DIR/bad.env"
  local output="$TMP_DIR/bad-manifest.out"
  make_stub_bin_dir "$stub_dir"
  write_common_stubs "$stub_dir" "1.81.0" "1.81.0"
  write_stellar_stub "$stub_dir" "21.0.0"

  cp "$ROOT_DIR/deploy/manifests/testnet.env" "$bad_manifest"
  echo 'SECRET_KEY="should-not-be-here"' >>"$bad_manifest"

  if PATH="$stub_dir:$PATH" bash "$ROOT_DIR/deploy.sh" --check-toolchain --manifest "$bad_manifest" testnet >"$output" 2>&1; then
    echo "expected bad manifest to fail" >&2
    exit 1
  fi

  assert_contains "$output" "unexpected manifest key 'SECRET_KEY'"
}

run_help_case
run_help_marker_integrity_case
run_success_case
run_rust_drift_case
run_cli_drift_case
run_manifest_rejects_unexpected_key_case

echo "deploy.sh manifest/toolchain tests passed"
