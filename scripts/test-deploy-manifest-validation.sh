#!/usr/bin/env bash
# Deterministic self-test for deploy/validate_manifest.sh. Exercises the
# success path and each rejection case (bad schema, out-of-bounds delay/
# cooldown/threshold, invalid service address, mainnet default identity)
# against fixture manifests. Makes no network calls.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
# shellcheck source=../deploy/validate_manifest.sh
source "$REPO_ROOT/deploy/validate_manifest.sh"

TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

VALID_ADMIN="operator"
VALID_SERVICE="GBZXN7PIRZGNMHGA7MUUUF4GWPJUEIYSAAWJKGH3PXWXOG7BXUZW7UMB"

write_manifest() {
  cat >"$TMP_DIR/manifest.json" <<EOF
{
  "testnet": { "upgrade_delay_secs": $1, "cooldown_secs": $2, "risk_threshold": $3, "schema_version": $4 },
  "mainnet": { "upgrade_delay_secs": $1, "cooldown_secs": $2, "risk_threshold": $3, "schema_version": $4 }
}
EOF
}

pass=0
fail=0

expect_ok() {
  local desc="$1"
  shift
  if "$@" >/dev/null 2>&1; then
    echo "ok   - $desc"
    pass=$((pass + 1))
  else
    echo "FAIL - $desc (expected success, got failure)"
    fail=$((fail + 1))
  fi
}

expect_fail() {
  local desc="$1"
  shift
  if "$@" >/dev/null 2>&1; then
    echo "FAIL - $desc (expected failure, got success)"
    fail=$((fail + 1))
  else
    echo "ok   - $desc"
    pass=$((pass + 1))
  fi
}

# Valid manifest, valid inputs.
write_manifest 172800 3600 75 4
expect_ok "valid manifest passes for testnet" \
  validate_manifest testnet "$TMP_DIR/manifest.json" "$VALID_ADMIN" "$VALID_SERVICE"
expect_ok "valid manifest passes for mainnet with non-default identity" \
  validate_manifest mainnet "$TMP_DIR/manifest.json" "$VALID_ADMIN" "$VALID_SERVICE"

# Schema mismatch.
write_manifest 172800 3600 75 99
expect_fail "stale schema_version is rejected" \
  validate_manifest testnet "$TMP_DIR/manifest.json" "$VALID_ADMIN" "$VALID_SERVICE"

# Upgrade delay below the on-chain minimum.
write_manifest 60 3600 75 4
expect_fail "upgrade_delay_secs below minimum is rejected" \
  validate_manifest testnet "$TMP_DIR/manifest.json" "$VALID_ADMIN" "$VALID_SERVICE"

# Cooldown above the on-chain maximum.
write_manifest 172800 999999 75 4
expect_fail "cooldown_secs above maximum is rejected" \
  validate_manifest testnet "$TMP_DIR/manifest.json" "$VALID_ADMIN" "$VALID_SERVICE"

# Risk threshold out of range.
write_manifest 172800 3600 150 4
expect_fail "risk_threshold above 100 is rejected" \
  validate_manifest testnet "$TMP_DIR/manifest.json" "$VALID_ADMIN" "$VALID_SERVICE"

# Invalid service address format.
write_manifest 172800 3600 75 4
expect_fail "malformed service address is rejected" \
  validate_manifest testnet "$TMP_DIR/manifest.json" "$VALID_ADMIN" "not-a-stellar-address"

# Mainnet refuses the default 'deployer' identity.
expect_fail "mainnet rejects the default 'deployer' admin identity" \
  validate_manifest mainnet "$TMP_DIR/manifest.json" "deployer" "$VALID_SERVICE"

# Missing network entry.
expect_fail "missing network entry in manifest is rejected" \
  validate_manifest devnet "$TMP_DIR/manifest.json" "$VALID_ADMIN" "$VALID_SERVICE"

echo ""
echo "$pass passed, $fail failed"
[ "$fail" -eq 0 ]
