#!/usr/bin/env bash
set -euo pipefail
cd "${CLAUDE_PROJECT_DIR:-.}"

# PostToolUse hook: auto-format, lint, and delegate fixes.
# Triggers for: .rs, .toml, .json, .md, .yaml, .yml
# Does NOT run tests — that's handled by intercept_test.sh

# ── Helpers ─────────────────────────────────────────────────

exit_ok() {
  echo '{}'
  exit 0
}

exit_block() {
  local reason="$1"
  local context="${2:-}"
  if command -v jq >/dev/null 2>&1; then
    if [[ -n "$context" ]]; then
      jq -n --arg r "$reason" --arg c "$context" \
        '{
          "decision": "block",
          "reason": $r,
          "hookSpecificOutput": {
            "hookEventName": "PostToolUse",
            "additionalContext": $c
          }
        }'
    else
      jq -n --arg r "$reason" \
        '{
          "decision": "block",
          "reason": $r
        }'
    fi
  else
    echo "{\"decision\":\"block\",\"reason\":\"$reason\"}"
  fi
  exit 0
}

# ── Guards ──────────────────────────────────────────────────

if ! command -v jq >/dev/null 2>&1; then
  exit_block "jq is required for the lint hook but is not installed. Install it with: brew install jq"
fi

input=$(cat)
file_path=$(echo "$input" | jq -r '.tool_input.file_path // empty' 2>/dev/null) || file_path=""
[[ -z "$file_path" ]] && exit_ok
[[ ! -f "$file_path" ]] && exit_ok

# ── File type gate ──────────────────────────────────────────

ext="${file_path##*.}"
case "$ext" in
  rs|toml|json|md|yaml|yml) ;;
  *) exit_ok ;;
esac

# ── Ensure required linters are installed ───────────────────

ensure_tool() {
  local cmd="$1" brew_pkg="$2"
  if ! command -v "$cmd" >/dev/null 2>&1; then
    if command -v brew >/dev/null 2>&1; then
      brew install "$brew_pkg" >/dev/null 2>&1 || true
    fi
  fi
}

case "$ext" in
  toml) ensure_tool taplo taplo ;;
  md)   ensure_tool markdownlint-cli2 markdownlint-cli2 ;;
  yaml|yml) ensure_tool yamllint yamllint ;;
esac

# ── Model tiering patterns (Rust-adapted from plankton) ─────
#
# Haiku: simple mechanical fixes (default)
# Sonnet: context-dependent fixes, markdown, yaml
# Opus: hardest type/lifetime errors, or high violation count
#
SONNET_PATTERN='clippy::cognitive_complexity|clippy::too_many_arguments|clippy::type_complexity|clippy::needless_lifetimes|E0277|E0308|E0412|E0433|MD[0-9]+|line-length|truthy|indentation'
OPUS_PATTERN='E0495|E0621|E0597|E0515|E0106|E0507|E0505|E0382'
VOLUME_THRESHOLD=5

# Unified violation collection
collected_violations="[]"

merge_violations() {
  local new="$1"
  collected_violations=$(echo "$collected_violations" "$new" | jq -s '.[0] + .[1]' 2>/dev/null) || true
}

# ════════════════════════════════════════════════════════════
# PHASE 1: Auto-format (silent)
# ════════════════════════════════════════════════════════════

run_phase1() {
  if [[ "$ext" == "rs" ]]; then
    cargo fmt --all 2>/dev/null || true
  fi

  if [[ "$ext" == "toml" ]] && command -v taplo >/dev/null 2>&1; then
    taplo fmt "$file_path" 2>/dev/null || true
  fi

  if [[ "$ext" == "md" ]] && command -v markdownlint-cli2 >/dev/null 2>&1; then
    markdownlint-cli2 --no-globs --fix "$file_path" 2>/dev/null || true
  fi

  if [[ "$ext" == "json" ]]; then
    if tmp=$(jq '.' "$file_path" 2>/dev/null); then
      echo "$tmp" > "$file_path"
    fi
  fi
}

run_phase1

# ════════════════════════════════════════════════════════════
# PHASE 2: Collect violations
# ════════════════════════════════════════════════════════════

run_phase2() {
  collected_violations="[]"

  # ── Rust: cargo clippy ──────────────────────────────────────

  if [[ "$ext" == "rs" ]] || [[ "$ext" == "toml" ]]; then
    # Bust clippy's incremental cache so it re-analyzes the edited file
    touch "$file_path" 2>/dev/null || true
    clippy_raw=$(cargo clippy --all-targets --all-features --message-format=json -- -D warnings 2>/dev/null) || true

    clippy_violations=$(echo "$clippy_raw" | \
      jq -c '
        select(.reason == "compiler-message")
        | select(.message.level == "error" or .message.level == "warning")
        | select(.message.code != null)
        | select(.message.spans | length > 0)
        | {
            file: .message.spans[0].file_name,
            line: .message.spans[0].line_start,
            column: .message.spans[0].column_start,
            code: .message.code.code,
            message: .message.message,
            linter: "clippy"
          }
      ' 2>/dev/null | jq -s '.' 2>/dev/null) || clippy_violations="[]"

    merge_violations "$clippy_violations"
  fi

  # ── TOML: taplo lint ────────────────────────────────────────

  if [[ "$ext" == "toml" ]] && command -v taplo >/dev/null 2>&1; then
    taplo_raw=$(taplo lint "$file_path" 2>&1) || true
    if [[ -n "$taplo_raw" ]] && echo "$taplo_raw" | grep -qi "error\|invalid"; then
      taplo_violations=$(jq -n --arg f "$file_path" --arg m "$taplo_raw" \
        '[{file: $f, line: 0, column: 0, code: "TOML_SYNTAX", message: $m, linter: "taplo"}]' \
        2>/dev/null) || taplo_violations="[]"
      merge_violations "$taplo_violations"
    fi
  fi

  # ── Markdown: markdownlint-cli2 ─────────────────────────────

  if [[ "$ext" == "md" ]] && command -v markdownlint-cli2 >/dev/null 2>&1; then
    mdlint_raw=$(markdownlint-cli2 --no-globs "$file_path" 2>&1) || true
    if [[ -n "$mdlint_raw" ]]; then
      md_violations=$(echo "$mdlint_raw" | while IFS= read -r line; do
        md_line=$(echo "$line" | grep -oE ':([0-9]+)' | head -1 | tr -d ':') || md_line="0"
        md_code=$(echo "$line" | grep -oE 'MD[0-9]+(/[a-z-]+)?' | head -1) || md_code=""
        md_msg=$(echo "$line" | sed -E 's/^[^:]+:[0-9]+ //' || echo "$line")
        [[ -n "$md_code" ]] && echo "{\"file\":\"$file_path\",\"line\":${md_line:-0},\"column\":0,\"code\":\"$md_code\",\"message\":$(echo "$md_msg" | jq -Rs .),\"linter\":\"markdownlint\"}"
      done | jq -s '.' 2>/dev/null) || md_violations="[]"
      merge_violations "$md_violations"
    fi
  fi

  # ── YAML: yamllint ──────────────────────────────────────────

  if [[ "$ext" == "yaml" ]] || [[ "$ext" == "yml" ]]; then
    if command -v yamllint >/dev/null 2>&1; then
      yaml_raw=$(yamllint -f parsable "$file_path" 2>&1) || true
      if [[ -n "$yaml_raw" ]]; then
        yaml_violations=$(echo "$yaml_raw" | while IFS= read -r line; do
          y_line=$(echo "$line" | sed -E 's/^[^:]*:([0-9]+):[0-9]+: .*/\1/' 2>/dev/null) || y_line="0"
          y_col=$(echo "$line" | sed -E 's/^[^:]*:[0-9]+:([0-9]+): .*/\1/' 2>/dev/null) || y_col="0"
          y_code=$(echo "$line" | grep -oE '\([^)]+\)' | tr -d '()' | head -1) || y_code="yaml-error"
          y_msg=$(echo "$line" | sed -E 's/^[^:]*:[0-9]+:[0-9]+: \[[a-z]+\] //' | sed -E 's/ \([^)]+\)$//' 2>/dev/null) || y_msg="$line"
          [[ -n "$y_code" ]] && echo "{\"file\":\"$file_path\",\"line\":${y_line:-0},\"column\":${y_col:-0},\"code\":\"$y_code\",\"message\":$(echo "$y_msg" | jq -Rs .),\"linter\":\"yamllint\"}"
        done | jq -s '.' 2>/dev/null) || yaml_violations="[]"
        merge_violations "$yaml_violations"
      fi
    fi
  fi

  # ── JSON: syntax check ──────────────────────────────────────

  if [[ "$ext" == "json" ]]; then
    if ! jq empty "$file_path" 2>/dev/null; then
      json_err=$(jq empty "$file_path" 2>&1 || true)
      json_violations=$(jq -n --arg f "$file_path" --arg m "$json_err" \
        '[{file: $f, line: 0, column: 0, code: "JSON_SYNTAX", message: $m, linter: "jq"}]' \
        2>/dev/null) || json_violations="[]"
      merge_violations "$json_violations"
    fi
  fi

  # ── Check violation count ───────────────────────────────────

  violation_count=$(echo "$collected_violations" | jq 'length' 2>/dev/null) || violation_count=0
  all_codes=""
  if [[ "$violation_count" -gt 0 ]]; then
    all_codes=$(echo "$collected_violations" | jq -r '[.[].code] | sort | unique | join(", ")' 2>/dev/null) || all_codes=""
  fi
}

run_phase2

if [[ "$violation_count" -eq 0 ]]; then
  exit_ok
fi
# ── Testing bypass ──────────────────────────────────────────

if [[ "${HOOK_SKIP_SUBPROCESS:-}" == "1" ]]; then
  exit_block "$violation_count violation(s): $all_codes. Fix them." "$all_codes"
fi

# ════════════════════════════════════════════════════════════
# PHASE 3: Delegate to subprocess
# ════════════════════════════════════════════════════════════

# ── Detect timeout command (GNU timeout or macOS gtimeout) ──

timeout_cmd=""
if command -v timeout >/dev/null 2>&1; then
  timeout_cmd="timeout"
elif command -v gtimeout >/dev/null 2>&1; then
  timeout_cmd="gtimeout"
fi

# ── Select model tier based on violation codes ──────────────

model="haiku"
max_turns=10
tier_timeout=120
allowed_tools="Edit Read"

if echo "$all_codes" | grep -qE "$SONNET_PATTERN"; then
  model="sonnet"
  max_turns=10
  tier_timeout=300
  allowed_tools="Edit Read"
fi

if echo "$all_codes" | grep -qE "$OPUS_PATTERN"; then
  model="opus"
  max_turns=15
  tier_timeout=600
  allowed_tools="Edit Read Write"
fi

# Volume override: many violations → opus
if [[ "$violation_count" -gt "$VOLUME_THRESHOLD" ]]; then
  model="opus"
  max_turns=15
  tier_timeout=600
  allowed_tools="Edit Read Write"
fi

# ── Build file-type-specific prompt ─────────────────────────

case "$ext" in
  rs)
    prompt="You are a code quality fixer. Fix ALL violations listed below in ${file_path}.

VIOLATIONS:
${collected_violations}

RULES:
1. Use targeted Edit operations only - never rewrite the entire file
2. Fix each violation at its reported line/column
3. The hook pipeline will auto-format and re-run validation after your edits
4. If a violation cannot be fixed, explain why
5. Do not use allow macro to bypass the problem. Fix it

Do not add comments explaining fixes. Do not refactor beyond what's needed."
    ;;

  toml)
    prompt="You are a code quality fixer. Fix ALL violations listed below in ${file_path}.

VIOLATIONS:
${collected_violations}

RULES:
1. Use targeted Edit operations only - never rewrite the entire file
2. Fix each violation at its reported line/column
3. The hook pipeline will auto-format and re-run validation after your edits
4. If a violation cannot be fixed, explain why

Do not add comments explaining fixes. Do not refactor beyond what's needed."
    ;;

  md)
    prompt="You are a markdown fixer. Fix ALL violations in ${file_path}.

VIOLATIONS:
${collected_violations}

MARKDOWN FIX STRATEGIES:
- MD013 (line length >80): SHORTEN content, don't wrap. Examples:
  - 'Skip delegation, report violations directly' -> 'Skip delegation, report directly'
  - 'Refactor to early returns, extract Config class' -> 'Refactor to early returns'
  - Remove redundant words: 'in order to' -> 'to', 'that is' -> ''
- MD060 (table style): Add spaces around ALL pipes in separator rows:
  - WRONG: |--------|------|
  - RIGHT: | ------ | ---- |
- Tables: When shortening, preserve meaning. Abbreviate consistently.

RULES:
1. Use targeted Edit operations - fix specific lines, never rewrite entire file
2. For tables: edit the ENTIRE row in one Edit to keep columns consistent
3. The hook pipeline will auto-format and re-run validation after your edits


Be concise. No explanations in the file."
    ;;

  yaml|yml)
    prompt="You are a code quality fixer. Fix ALL violations listed below in ${file_path}.

VIOLATIONS:
${collected_violations}

RULES:
1. Use targeted Edit operations only - never rewrite the entire file
2. Fix each violation at its reported line/column
3. The hook pipeline will auto-format and re-run validation after your edits
4. If a violation cannot be fixed, explain why

Do not add comments explaining fixes. Do not refactor beyond what's needed."
    ;;

  json)
    prompt="You are a code quality fixer. Fix ALL violations listed below in ${file_path}.

VIOLATIONS:
${collected_violations}

RULES:
1. Use targeted Edit operations only - never rewrite the entire file
2. Fix each violation at its reported line/column
3. The hook pipeline will auto-format and re-run validation after your edits
4. If a violation cannot be fixed, explain why

Do not add comments explaining fixes. Do not refactor beyond what's needed."
    ;;
esac

# ── Spawn subprocess ────────────────────────────────────────

claude_cmd=""
if command -v claude >/dev/null 2>&1; then
  claude_cmd="claude"
fi

# CLAUDE_CONFIG_DIR inherited from parent process (user's claudio fish function)

phase3_rc=0

if [[ -n "$claude_cmd" ]]; then
  settings_file=".claude/subprocess-settings.json"
  if [[ ! -f "$settings_file" ]]; then
    echo '{"disableAllHooks":true,"skipDangerousModePermissionPrompt":true}' > "$settings_file"
  fi

  # Derive disallowed tools: full universe minus allowed
  all_tools="Edit,Read,Write,Bash,Glob,Grep,WebFetch,WebSearch,NotebookEdit,Task,AskUserQuestion,EnterPlanMode,ExitPlanMode"
  disallowed_tools=""
  IFS=',' read -ra all_arr <<< "$all_tools"
  for t in "${all_arr[@]}"; do
    if ! echo "$allowed_tools" | grep -qw "$t"; then
      disallowed_tools="${disallowed_tools:+$disallowed_tools,}$t"
    fi
  done

  # Build subprocess command with optional timeout wrapper
  subprocess_cmd=(env -u CLAUDECODE "$claude_cmd" -p "$prompt"
    --dangerously-skip-permissions
    --setting-sources ""
    --settings "$settings_file"
    --disallowedTools "$disallowed_tools"
    --max-turns "$max_turns"
    --model "$model"
    --effort medium
    "$file_path")

  # Recursion prevention:
  # 1. env -u CLAUDECODE — no-op in hooks (not set), kept for non-hook invocations
  # 2. --setting-sources "" — don't load project settings.local.json (has hooks config)
  # 3. --settings with disableAllHooks: true — explicitly disable hooks in subprocess
  # stdout discarded; stderr flows through for observability
  if [[ -n "$timeout_cmd" ]]; then
    "$timeout_cmd" "$tier_timeout" "${subprocess_cmd[@]}" >/dev/null || phase3_rc=$?
  else
    "${subprocess_cmd[@]}" >/dev/null || phase3_rc=$?
  fi
fi

# ════════════════════════════════════════════════════════════
# PHASE 4: Re-verify (re-run Phase 1 + Phase 2)
# ════════════════════════════════════════════════════════════

run_phase1
run_phase2

if [[ "$violation_count" -gt 0 ]]; then
  reason="$violation_count violation(s): $all_codes. Fix them."
  [[ "$phase3_rc" -ne 0 ]] && reason="(autofix errored) $reason"
  exit_block "$reason" "$all_codes"
fi

exit_ok
