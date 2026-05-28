#!/usr/bin/env bash
# Intent citation: docs/architecture/ADR-003-engineering-standards.md
# Feature: engineer-backtest-mode — Pre-merge regression gate hook
#
# This script invokes the backtest regression gate before a merge is allowed.
# It runs the full backtest suite and blocks the merge if any behavioral
# contracts fail.
#
# Usage:
#   ./scripts/backtest-gate.sh [--timeout <ms>] [--node <node-id>]
#
# Exit codes:
#   0 - All contracts pass, merge allowed
#   1 - Regression detected, merge blocked
#   2 - Script error (configuration, timeout, etc.)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Defaults
TIMEOUT_MS="${BACKTEST_TIMEOUT_MS:-300000}"
NODE_ID="${BACKTEST_NODE_ID:-compute-desktop-local}"
SUITE_TYPES="${BACKTEST_SUITES:-vitest,cargo-test}"

# Parse arguments
while [[ $# -gt 0 ]]; do
  case "$1" in
    --timeout)
      TIMEOUT_MS="$2"
      shift 2
      ;;
    --node)
      NODE_ID="$2"
      shift 2
      ;;
    --suites)
      SUITE_TYPES="$2"
      shift 2
      ;;
    --help|-h)
      echo "Usage: $0 [--timeout <ms>] [--node <node-id>] [--suites <types>]"
      echo ""
      echo "Options:"
      echo "  --timeout <ms>     Execution timeout in milliseconds (default: 300000)"
      echo "  --node <node-id>   Target compute node (default: compute-desktop-local)"
      echo "  --suites <types>   Comma-separated suite types (default: vitest,cargo-test)"
      echo ""
      echo "Environment variables:"
      echo "  BACKTEST_TIMEOUT_MS   Same as --timeout"
      echo "  BACKTEST_NODE_ID      Same as --node"
      echo "  BACKTEST_SUITES       Same as --suites"
      exit 0
      ;;
    *)
      echo "ERROR: Unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

echo "╔══════════════════════════════════════════════════════════════╗"
echo "║  ResonantOS vNext — Backtest Regression Gate                ║"
echo "╚══════════════════════════════════════════════════════════════╝"
echo ""
echo "  Node:    $NODE_ID"
echo "  Suites:  $SUITE_TYPES"
echo "  Timeout: ${TIMEOUT_MS}ms"
echo "  Branch:  $(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo 'unknown')"
echo "  Commit:  $(git rev-parse --short HEAD 2>/dev/null || echo 'unknown')"
echo ""

# Run the vitest backtest suite
echo "▶ Running backtest regression gate..."
cd "$PROJECT_ROOT"

# Execute the backtest suite via npx vitest
IFS=',' read -ra SUITES <<< "$SUITE_TYPES"
GATE_PASSED=true
FAIL_COUNT=0
PASS_COUNT=0

for suite in "${SUITES[@]}"; do
  echo ""
  echo "  ▸ Suite: $suite"

  case "$suite" in
    vitest)
      if npx vitest run --reporter=dot 2>&1; then
        echo "    ✓ vitest suite passed"
        PASS_COUNT=$((PASS_COUNT + 1))
      else
        echo "    ✗ vitest suite FAILED"
        GATE_PASSED=false
        FAIL_COUNT=$((FAIL_COUNT + 1))
      fi
      ;;
    cargo-test)
      if [ -d "$PROJECT_ROOT/src-tauri" ]; then
        if (cd "$PROJECT_ROOT/src-tauri" && cargo test 2>&1); then
          echo "    ✓ cargo-test suite passed"
          PASS_COUNT=$((PASS_COUNT + 1))
        else
          echo "    ✗ cargo-test suite FAILED"
          GATE_PASSED=false
          FAIL_COUNT=$((FAIL_COUNT + 1))
        fi
      else
        echo "    ⊘ cargo-test skipped (no src-tauri directory)"
      fi
      ;;
    *)
      echo "    ⊘ Unknown suite type: $suite (skipped)"
      ;;
  esac
done

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

if [ "$GATE_PASSED" = true ]; then
  echo "✓ REGRESSION GATE PASSED ($PASS_COUNT suite(s) passed, $FAIL_COUNT failed)"
  echo "  Merge is allowed."
  exit 0
else
  echo "✗ REGRESSION GATE BLOCKED ($PASS_COUNT suite(s) passed, $FAIL_COUNT failed)"
  echo "  Merge is NOT allowed. Fix failing contracts before merging."
  exit 1
fi
