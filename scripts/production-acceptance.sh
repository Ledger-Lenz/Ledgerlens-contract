#!/usr/bin/env bash
# Run the production acceptance suite and write an auditable readiness report.
#
# Usage:
#   scripts/production-acceptance.sh [--output docs/reports/production-readiness-report.md]
#
# The script is intentionally local-network agnostic. Live testnet canary drills
# are handled by scripts/testnet-canary-rehearsal.sh and their report path is
# referenced from the readiness report.
set -euo pipefail

OUTPUT="docs/reports/production-readiness-report.md"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --output)
      OUTPUT="${2:?missing value for --output}"
      shift 2
      ;;
    -h|--help)
      sed -n '2,14p' "$0"
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

export PATH="$HOME/.cargo/bin:$PATH"

mkdir -p "$(dirname "$OUTPUT")"

RUN_ID="${RUN_ID:-prod-acceptance-$(date -u +%Y%m%dT%H%M%SZ)}"
LOG_DIR="target/production-acceptance/$RUN_ID"
mkdir -p "$LOG_DIR"

REPORT_TMP="$LOG_DIR/report.md"
WASM_PATH="target/wasm32-unknown-unknown/release/ledgerlens_score.wasm"
STATUS=0

for required in cargo sha256sum wc; do
  if ! command -v "$required" >/dev/null 2>&1; then
    echo "missing required command: $required" >&2
    exit 127
  fi
done

publish_partial_report() {
  if [[ -f "$REPORT_TMP" ]]; then
    cp "$REPORT_TMP" "$OUTPUT"
  fi
}

trap publish_partial_report EXIT

append() {
  printf '%s\n' "$*" >> "$REPORT_TMP"
}

run_step() {
  local id="$1"
  local desc="$2"
  shift 2
  local log="$LOG_DIR/$id.log"

  append "### $id - $desc"
  append ""
  append '```text'
  append "$*"
  append '```'
  append ""

  set +e
  "$@" > "$log" 2>&1
  local code=$?
  set -e

  if [[ "$code" -eq 0 ]]; then
    append "Result: PASS"
  else
    append "Result: FAIL (exit $code)"
    STATUS=1
  fi
  append "Log: $log"
  append ""
}

{
  echo "# Production Readiness Report"
  echo ""
  echo "Run ID: $RUN_ID"
  echo "Generated UTC: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "Repository: Ledger-Lenz/Ledgerlens-contract"
  echo "Commit: $(git rev-parse HEAD 2>/dev/null || echo unknown)"
  echo "Branch: $(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo unknown)"
  echo "Operator: ${READY_SIGNER_NAME:-UNSIGNED}"
  echo "Decision authority: ${READY_DECISION_AUTHORITY:-UNSIGNED}"
  echo "Canary rehearsal report: ${CANARY_REHEARSAL_REPORT:-not-attached}"
  echo ""
  echo "## Readiness Decision"
  echo ""
  echo "- Status: ${READY_STATUS:-PENDING_REVIEW}"
  echo "- Signature: ${READY_SIGNATURE:-UNSIGNED}"
  echo "- Signed at UTC: ${READY_SIGNED_AT:-UNSIGNED}"
  echo ""
  echo "## Acceptance Evidence"
  echo ""
} > "$REPORT_TMP"

run_step "fmt" "Rust formatting is stable" cargo fmt --all -- --check
run_step "clippy" "Strict linting has no warnings" cargo clippy --all-targets -- -D warnings
run_step "workspace-tests" "Native workspace tests pass" cargo test --workspace
run_step "replay" "Deterministic replay harness passes" cargo test -p replay
run_step "wasm-release-build" "Locked release WASM builds" cargo build --target wasm32-unknown-unknown --release -p ledgerlens-score --locked

if [[ -f "$WASM_PATH" ]]; then
  WASM_SIZE="$(wc -c < "$WASM_PATH" | tr -d ' ')"
  WASM_SHA="$(sha256sum "$WASM_PATH" | awk '{print $1}')"
else
  WASM_SIZE="missing"
  WASM_SHA="missing"
  STATUS=1
fi

append "## Artifact Evidence"
append ""
append "- WASM path: \`$WASM_PATH\`"
append "- WASM bytes: \`$WASM_SIZE\`"
append "- WASM sha256: \`$WASM_SHA\`"
append "- Error discriminants: append-only check is enforced in PR CI by \`tools/check_error_discriminants.sh\`."
append "- Storage/ABI change: this acceptance suite adds tooling and documentation only; no contract ABI, error discriminant, event, or storage-key change is expected."
append ""

if command -v twiggy >/dev/null 2>&1 && [[ -f "$WASM_PATH" ]]; then
  run_step "wasm-size-report" "WASM size contributors are measured" bash scripts/wasm-size-report.sh --output "$LOG_DIR/wasm-size-report.md"
  append "WASM size report: $LOG_DIR/wasm-size-report.md"
  append ""
else
  append "WASM size report: skipped because \`twiggy\` is not installed. Byte size and sha256 are still recorded above."
  append ""
fi

append "## Operational Evidence Required Before Mainnet Activation"
append ""
append "- Testnet canary and failure-injection report from \`scripts/testnet-canary-rehearsal.sh\`."
append "- Backup/restore or rollback drill evidence with state digest reconciliation."
append "- Runbook reviewer name, review date, and issues found or waived."
append "- Monitoring thresholds confirmed for submissions, pause state, upgrade proposals, signer rotation, stale data, and replay failures."
append ""
append "## Final Status"
append ""
if [[ "$STATUS" -eq 0 ]]; then
  append "Automated acceptance checks passed. Human sign-off remains required unless signature fields above are populated."
else
  append "Automated acceptance checks failed. Production activation is blocked."
fi

cp "$REPORT_TMP" "$OUTPUT"
if [[ -n "${READY_GPG_KEY:-}" ]]; then
  if ! command -v gpg >/dev/null 2>&1; then
    echo "READY_GPG_KEY is set but gpg is not installed" >&2
    exit 127
  fi
  gpg --batch --yes --local-user "$READY_GPG_KEY" --detach-sign --armor "$OUTPUT"
fi
echo "production readiness report written to $OUTPUT"
exit "$STATUS"
