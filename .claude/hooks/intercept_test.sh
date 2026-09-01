#!/usr/bin/env bash
set -euo pipefail
cd "${CLAUDE_PROJECT_DIR:-.}"

# PreToolUse hook on Bash: intercepts cargo test/nextest commands
# and replaces them with our test_runner.sh for clean output.

input=$(cat)

if ! command -v jq >/dev/null 2>&1; then
  echo '{}'
  exit 0
fi

command_str=$(echo "$input" | jq -r '.tool_input.command // empty' 2>/dev/null) || command_str=""
[[ -z "$command_str" ]] && { echo '{}'; exit 0; }

# Check if command is a cargo test/nextest invocation
if echo "$command_str" | grep -qE '(^|\s|&&|\|)(cargo\s+(test|nextest))'; then
  # Replace the command with our test runner
  jq -n '{
    "hookSpecificOutput": {
      "hookEventName": "PreToolUse",
      "updatedInput": {
        "command": ".claude/hooks/test_runner.sh"
      }
    }
  }'
  exit 0
fi

# Not a test command — approve as-is
echo '{}'
exit 0
