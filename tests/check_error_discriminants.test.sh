#!/usr/bin/env bash
# Direct tests for tools/check_error_discriminants.sh.
#
# The checker compares the `Error` enum discriminants in
# contracts/ledgerlens-score/src/errors.rs at two git refs and fails if any
# existing discriminant was renamed, removed, or renumbered (the append-only
# rule in CONTRIBUTING.md). These tests build a scratch git repo with a
# controlled errors.rs history and assert the checker both accepts legitimate
# append-only changes and rejects every way of disturbing an existing
# discriminant.
#
# Usage: tests/check_error_discriminants.test.sh

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CHECKER="$ROOT_DIR/tools/check_error_discriminants.sh"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

# ── Scratch repo with a canned errors.rs history ──────────────────────────
REPO="$TMP_DIR/repo"
ERRORS="$REPO/contracts/ledgerlens-score/src/errors.rs"
mkdir -p "$(dirname "$ERRORS")"
git -C "$REPO" init -q
git -C "$REPO" config user.email "checker-test@example.com"
git -C "$REPO" config user.name "Checker Test"

write_errors() {
  cat > "$ERRORS"
}

commit_state() {
  git -C "$REPO" add -A
  git -C "$REPO" commit -q -m "$1"
  git -C "$REPO" rev-parse HEAD
}

# Canned base state: 4 discriminants plus `pub const` aliases, mirroring the
# shape of the real errors.rs (aliases must never be mistaken for variants).
write_errors <<'EOF'
use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    InvalidScore = 4,
}

impl Error {
    pub const InvalidMinConfidence: Error = Error::InvalidScore;
    pub const InvalidWithdrawalAmount: Error = Error::Unauthorized;
}
EOF

BASE_SHA="$(commit_state base)"

# ── Helpers ────────────────────────────────────────────────────────────────
pass=0
fail=0
LAST_OUTPUT=""

# run_checker <desc> <expected-exit> <base-ref> <head-ref>
run_checker() {
  local desc="$1" expected="$2" base="$3" head="$4"
  local actual=0
  set +e
  LAST_OUTPUT="$(cd "$REPO" && bash "$CHECKER" "$base" "$head" 2>&1)"
  actual=$?
  set -e
  if [ "$actual" -eq "$expected" ]; then
    pass=$((pass + 1))
  else
    fail=$((fail + 1))
    echo "FAIL: $desc (expected exit $expected, got $actual)"
    printf '%s\n' "$LAST_OUTPUT"
  fi
}

assert_output_contains() {
  local needle="$1"
  if grep -Fq "$needle" <<<"$LAST_OUTPUT"; then
    pass=$((pass + 1))
  else
    fail=$((fail + 1))
    echo "FAIL: expected output to contain '$needle'"
    printf '%s\n' "$LAST_OUTPUT"
  fi
}

# ── Append-only change: adding a new discriminant is allowed ───────────────
write_errors <<'EOF'
use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    InvalidScore = 4,
    ScoreNotFound = 5,
}

impl Error {
    pub const InvalidMinConfidence: Error = Error::InvalidScore;
    pub const InvalidWithdrawalAmount: Error = Error::Unauthorized;
}
EOF
HEAD_SHA="$(commit_state append)"
run_checker "appending a new discriminant passes" 0 "$BASE_SHA" "$HEAD_SHA"
assert_output_contains "Error discriminant stability check passed (1 new discriminant(s) added)."

# ── Renaming an existing discriminant is rejected ──────────────────────────
write_errors <<'EOF'
use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Forbidden = 3,
    InvalidScore = 4,
}

impl Error {
    pub const InvalidMinConfidence: Error = Error::InvalidScore;
    pub const InvalidWithdrawalAmount: Error = Error::Unauthorized;
}
EOF
HEAD_SHA="$(commit_state rename)"
run_checker "renaming an existing discriminant fails" 1 "$BASE_SHA" "$HEAD_SHA"
assert_output_contains "discriminant 3 changed variant name from 'Unauthorized' to 'Forbidden'"

# ── Removing an existing discriminant is rejected ──────────────────────────
write_errors <<'EOF'
use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    InvalidScore = 4,
}

impl Error {
    pub const InvalidMinConfidence: Error = Error::InvalidScore;
    pub const InvalidWithdrawalAmount: Error = Error::Unauthorized;
}
EOF
HEAD_SHA="$(commit_state remove)"
run_checker "removing an existing discriminant fails" 1 "$BASE_SHA" "$HEAD_SHA"
assert_output_contains "discriminant 3 (Unauthorized) was removed or renumbered"

# ── Renumbering an existing discriminant is rejected ───────────────────────
write_errors <<'EOF'
use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 99,
    InvalidScore = 4,
}

impl Error {
    pub const InvalidMinConfidence: Error = Error::InvalidScore;
    pub const InvalidWithdrawalAmount: Error = Error::Unauthorized;
}
EOF
HEAD_SHA="$(commit_state renumber)"
run_checker "renumbering an existing discriminant fails" 1 "$BASE_SHA" "$HEAD_SHA"
assert_output_contains "discriminant 3 (Unauthorized) was removed or renumbered"

# ── Swapping two discriminants is rejected ─────────────────────────────────
write_errors <<'EOF'
use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 2,
    NotInitialized = 1,
    Unauthorized = 3,
    InvalidScore = 4,
}

impl Error {
    pub const InvalidMinConfidence: Error = Error::InvalidScore;
    pub const InvalidWithdrawalAmount: Error = Error::Unauthorized;
}
EOF
HEAD_SHA="$(commit_state swap)"
run_checker "swapping two discriminants fails" 1 "$BASE_SHA" "$HEAD_SHA"
assert_output_contains "discriminant 1 changed variant name from 'AlreadyInitialized' to 'NotInitialized'"

# ── Adding only `pub const` aliases is allowed ─────────────────────────────
write_errors <<'EOF'
use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    InvalidScore = 4,
}

impl Error {
    pub const InvalidMinConfidence: Error = Error::InvalidScore;
    pub const InvalidWithdrawalAmount: Error = Error::Unauthorized;
    pub const PairPaused: Error = Error::Unauthorized;
    pub const DelegateNotFound: Error = Error::InvalidScore;
}
EOF
HEAD_SHA="$(commit_state aliases)"
run_checker "adding only pub const aliases passes" 0 "$BASE_SHA" "$HEAD_SHA"
assert_output_contains "Error discriminant stability check passed (0 new discriminant(s) added)."

# ── Base ref without the file is skipped, not failed ───────────────────────
git -C "$REPO" rm -q contracts/ledgerlens-score/src/errors.rs
NO_FILE_SHA="$(commit_state remove-errors-file)"
run_checker "missing errors.rs at base is skipped" 0 "$NO_FILE_SHA" "$BASE_SHA"
assert_output_contains "found no discriminants in contracts/ledgerlens-score/src/errors.rs"

# ── Base ref with an empty enum is skipped, not failed ─────────────────────
mkdir -p "$(dirname "$ERRORS")"
printf 'pub enum Error {}\n' > "$ERRORS"
EMPTY_SHA="$(commit_state empty-enum)"
run_checker "empty Error enum at base is skipped" 0 "$EMPTY_SHA" "$BASE_SHA"
assert_output_contains "found no discriminants in contracts/ledgerlens-score/src/errors.rs"

echo ""
echo "$pass passed, $fail failed"
[ "$fail" -eq 0 ]
