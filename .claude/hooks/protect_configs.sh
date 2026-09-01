#!/usr/bin/env bash
set -euo pipefail
cd "${CLAUDE_PROJECT_DIR:-.}"

# PreToolUse hook: blocks edits to linter configs and hook infrastructure.
# Fail-closed — if jq is missing or parsing fails, block the edit.

input=$(cat)

# --- Deny helper ---
deny() {
  local name="$1"
  if command -v jq >/dev/null 2>&1; then
    jq -n --arg reason "Protected config file ($name). Fix the code, not the rules." \
      '{
        "hookSpecificOutput": {
          "hookEventName": "PreToolUse",
          "permissionDecision": "deny",
          "permissionDecisionReason": $reason
        }
      }'
  else
    cat <<'BLOCK'
{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"Cannot verify file safety (jq missing). Edit blocked."}}
BLOCK
  fi
  exit 0
}

# Fail-closed: if jq is missing, block
if ! command -v jq >/dev/null 2>&1; then
  deny "unknown (jq missing)"
fi

file_path=$(echo "$input" | jq -r '.tool_input.file_path // empty' 2>/dev/null) || {
  deny "unknown (parse error)"
}

if [[ -z "$file_path" ]]; then
  echo '{}'
  exit 0
fi

# --- Protected .claude/ paths (NOT settings.json — it has no hook config) ---
case "$file_path" in
  .claude/hooks/*|*/.claude/hooks/*)       deny "$(basename "$file_path")" ;;
  .claude/settings.local.json|*/.claude/settings.local.json) deny "settings.local.json" ;;
  .claude/subprocess-settings.json|*/.claude/subprocess-settings.json) deny "subprocess-settings.json" ;;
esac

# --- Protected linter config basenames ---
basename_f=$(basename "$file_path")

PROTECTED_CONFIGS=(
  "rustfmt.toml"
  ".rustfmt.toml"
  "clippy.toml"
  ".clippy.toml"
  "deny.toml"
)

for cfg in "${PROTECTED_CONFIGS[@]}"; do
  if [[ "$basename_f" == "$cfg" ]]; then
    deny "$basename_f"
  fi
done

# --- Protected by path pattern ---
case "$file_path" in
  .config/nextest.toml|*/.config/nextest.toml) deny "nextest.toml" ;;
esac

# --- Approved ---
echo '{}'
exit 0
