#!/usr/bin/env bash
set -euo pipefail
cd "${CLAUDE_PROJECT_DIR:-.}"

# Runs cargo nextest run --fail-fast and returns either:
# - "All N tests passed" on success
# - The single failed test with its output on failure

output=$(cargo nextest run --fail-fast 2>&1) && exit_code=0 || exit_code=$?

# Extract summary line: "Summary [  10.357s] 1345 tests run: 1345 passed, 0 skipped"
summary=$(echo "$output" | grep -E '^\s*Summary' || echo "")

if [[ $exit_code -eq 0 ]]; then
  # All tests passed — extract count from summary
  if [[ -n "$summary" ]]; then
    echo "$summary"
  else
    echo "All tests passed."
  fi
else
  # Extract the FAIL line(s) — nextest format: "    FAIL [  0.123s] crate::module test_name"
  fail_line=$(echo "$output" | grep -E '^\s*FAIL' | head -1 || echo "")

  if [[ -n "$fail_line" ]]; then
    # Extract test name from FAIL line
    test_name=$(echo "$fail_line" | sed 's/.*FAIL\s*\[[^]]*\]\s*//')

    echo "FAILED: $test_name"
    echo ""

    # Show the test's stderr output (between --- STDOUT/STDERR markers)
    # nextest outputs test failure details before the FAIL summary
    echo "$output" | grep -A 50 -- "--- STDERR" | head -30 || true
  else
    # Fallback: show last 20 lines of output
    echo "Test run failed:"
    echo "$output" | tail -20
  fi

  if [[ -n "$summary" ]]; then
    echo ""
    echo "$summary"
  fi
fi
