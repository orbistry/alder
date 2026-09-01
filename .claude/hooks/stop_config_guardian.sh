#!/usr/bin/env bash
set -euo pipefail
cd "${CLAUDE_PROJECT_DIR:-.}"

# Stop hook: defense-in-depth check for modified config files.
# Uses jj diff to detect changes that slipped past PreToolUse.
# (jj has no staging area, so one check covers everything)

input=$(cat)

if ! command -v jq >/dev/null 2>&1; then
  echo '{"decision":"block","reason":"jq is required for config guardian but is not installed."}'
  exit 0
fi

# Prevent infinite loop
stop_hook_active=$(echo "$input" | jq -r '.stop_hook_active // false' 2>/dev/null) || stop_hook_active="false"
if [[ "$stop_hook_active" == "true" ]]; then
  echo '{}'
  exit 0
fi

# Protected file patterns
# Note: settings.local.json is gitignored, protected only by PreToolUse
PROTECTED_PATTERNS=(
  ".claude/hooks/"
  ".claude/subprocess-settings.json"
  "rustfmt.toml"
  ".rustfmt.toml"
  "clippy.toml"
  ".clippy.toml"
  ".config/nextest.toml"
  "deny.toml"
)

# Get all changed files in the working copy via jj
all_changed=$(jj diff --name-only 2>/dev/null) || all_changed=""

modified_files=()
for pattern in "${PROTECTED_PATTERNS[@]}"; do
  while IFS= read -r f; do
    [[ -z "$f" ]] && continue
    # Match pattern: exact match or prefix match (for directory patterns like .claude/hooks/)
    if [[ "$f" == "$pattern"* ]] || [[ "$f" == "$pattern" ]]; then
      modified_files+=("$f")
    fi
  done <<< "$all_changed"
done

# Deduplicate
if [[ ${#modified_files[@]} -gt 0 ]]; then
  deduped=()
  while IFS= read -r line; do
    [[ -n "$line" ]] && deduped+=("$line")
  done < <(printf '%s\n' "${modified_files[@]}" | sort -u)
  modified_files=("${deduped[@]}")
fi

if [[ ${#modified_files[@]} -eq 0 ]]; then
  echo '{}'
  exit 0
fi

files_list=$(printf '%s ' "${modified_files[@]}")
files_display=$(printf '  - %s\n' "${modified_files[@]}")

jq -n \
  --arg reason "Protected config files were modified during this session:
$files_display
Run \`jj restore $files_list\` to restore, or confirm the changes are intentional." \
  '{
    "decision": "block",
    "reason": $reason
  }'

exit 0
