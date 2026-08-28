#!/usr/bin/env bash
set -euo pipefail

# Find budget and tolerance from docs
DOC="docs/wasm-size-budget.md"
WASM_PATH="target/wasm32-unknown-unknown/release/ledgerlens_score.wasm"

BUDGET_STR=$(grep "\- \*\*Total Binary Size\*\*:" "$DOC" | grep -oE '[0-9,]+ bytes' | grep -oE '[0-9,]+' | tr -d ',')
TOLERANCE_STR=$(grep "\- \*\*Tolerance\*\*:" "$DOC" | grep -oE '[0-9.]+%' | tr -d '%' || echo "0")

if [[ -z "$BUDGET_STR" ]]; then
  echo "Error: Could not extract budget from $DOC"
  exit 1
fi

BUDGET=$BUDGET_STR
TOLERANCE=${TOLERANCE_STR:-0}

if [[ ! -f "$WASM_PATH" ]]; then
  echo "WASM binary not found at $WASM_PATH. Building..."
  cargo build --target wasm32-unknown-unknown --release -p ledgerlens-score --locked
fi

ACTUAL=$(wc -c < "$WASM_PATH" | tr -d ' ')

MAX_ALLOWED=$(awk "BEGIN {print int($BUDGET * (1 + $TOLERANCE / 100))}")

echo "WASM Size Check:"
echo "  Actual: $ACTUAL bytes"
echo "  Budget: $BUDGET bytes"
echo "  Tolerance: $TOLERANCE%"
echo "  Max Allowed: $MAX_ALLOWED bytes"

if [[ "$ACTUAL" -gt "$MAX_ALLOWED" ]]; then
  DELTA=$((ACTUAL - BUDGET))
  echo ""
  echo "❌ ERROR: WASM size budget exceeded!"
  echo "Actual size ($ACTUAL bytes) exceeds the budget ($BUDGET bytes) + tolerance ($TOLERANCE%) by $((ACTUAL - MAX_ALLOWED)) bytes."
  echo "Delta from baseline: +$DELTA bytes."
  echo ""
  
  if ! command -v twiggy &> /dev/null; then
    echo "Installing twiggy for size analysis..."
    cargo install twiggy || echo "⚠️ WARNING: twiggy installation failed, skipping detailed breakdown."
  fi

  if command -v twiggy &> /dev/null; then
    echo "Detailed breakdown of current size:"
    ./scripts/wasm-size-report.sh --wasm "$WASM_PATH" --top 15 || echo "⚠️ WARNING: Detailed breakdown script failed."
  else
    echo "⚠️ WARNING: twiggy unavailable, skipping detailed breakdown."
  fi
  echo ""
  echo "To bypass this gate, you must explicitly update the budget in docs/wasm-size-budget.md and get it reviewed."
  exit 1
else
  echo "✅ WASM size is within budget."
fi
