#!/usr/bin/env bash
# quality-gate.sh — Agent-agnostic quality gate for Rust projects
# Source: /home/feanor/Projects/.claude/scripts/quality-gate.sh
# Used by: BOTH Hermes and Claude Code
#
# Usage: quality-gate.sh [--project NAME] [--log] [--json] [--full]
#   --project NAME  Override project name (default: basename of CWD)
#   --log           Log results to ~/.grounded/session-log.jsonl
#   --json          Output results as JSON
#   --full          Scan ALL src/ files (not just diff) for file-level checks
#
# Checks (in order):
#   1. cargo fmt --check     (BLOCK)
#   2. cargo check           (BLOCK on error, WARN on warnings)
#   3. cargo test --lib      (BLOCK)
#   4. stub scan             (BLOCK) — todo!/unimplemented!/panic! in non-test
#   5. unwrap/expect scan    (WARN)  — in non-test code (M-ALLOW/M-UNWRAP markers exempt)
#   6. dead_code scan        (BLOCK) — #[allow(dead_code)] + #![allow(dead_code)]
#   7. allow-without-reason  (WARN)  — #[allow(...)] without reason=
#   8. bad comments scan     (BLOCK) — FIXME/HACK/XXX/TEMPORARY
#
# Exit codes: 0=clean, 1=blocked, 2=warnings only

set -euo pipefail

# ── Arg parsing (proper while/shift) ──
PROJECT=""
DO_LOG=false
DO_JSON=false
DO_FULL=false
SKIP_COUNT=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        --project) shift; PROJECT="${1:-}"; shift ;;
        --log)     DO_LOG=true; shift ;;
        --json)    DO_JSON=true; shift ;;
        --full)    DO_FULL=true; shift ;;
        *)         shift ;;
    esac
done

PROJECT="${PROJECT:-$(basename "$(pwd)")}"
TIMESTAMP=$(date -Iseconds)
LOGFILE="$HOME/.grounded/session-log.jsonl"
CWD="$(pwd)"

# Ensure ~/.grounded exists
mkdir -p "$HOME/.grounded"

# Track results
declare -a BLOCKS=()
declare -a WARNS=()
declare -a DETAILS=()

add_block() { BLOCKS+=("$1"); DETAILS+=("$1"); }
add_warn()  { WARNS+=("$1"); DETAILS+=("WARN: $1"); }
add_skip()  { SKIP_COUNT=$((SKIP_COUNT + 1)); DETAILS+=("SKIP: $1"); }

echo "══════════════════════════════════════════════"
echo "  QUALITY GATE: $PROJECT"
if [ "$DO_FULL" = true ]; then
    echo "  Mode: FULL SCAN (all src/ files)"
else
    echo "  Mode: DIFF-ONLY (modified files only)"
fi
echo "  Agent: $(whoami) | $(date +%H:%M:%S)"
echo "══════════════════════════════════════════════"
echo ""

# ── 1. cargo fmt ──
echo -n "  [1/8] cargo fmt --check... "
if cargo fmt --all -- --check 2>/dev/null; then
    echo "PASS"
else
    echo "FAIL"
    add_block "cargo fmt -- code is not formatted"
fi

# ── 2. cargo check (warns on compiler warnings) ──
echo -n "  [2/8] cargo check... "
CHECK_OUTPUT=$(cargo check 2>&1)
CHECK_EXIT=$?
CHECK_WARN_COUNT=$(echo "$CHECK_OUTPUT" | grep -c "^warning:" || true)
if [ $CHECK_EXIT -ne 0 ]; then
    echo "FAIL ($CHECK_WARN_COUNT warnings + errors)"
    add_block "cargo check failed with errors"
    echo "$CHECK_OUTPUT" | tail -10
elif [ "$CHECK_WARN_COUNT" -gt 0 ]; then
    echo "WARN ($CHECK_WARN_COUNT compiler warnings)"
    add_warn "$CHECK_WARN_COUNT compiler warning(s) -- run 'cargo check' to see them"
else
    echo "PASS"
fi

# ── 3. cargo test ──
echo -n "  [3/8] cargo test --lib... "
TEST_OUTPUT=$(cargo test --lib 2>&1 | tail -5)
if echo "$TEST_OUTPUT" | grep -q "FAILED\|^error"; then
    echo "FAIL"
    add_block "cargo test --lib failed"
    echo "$TEST_OUTPUT"
else
    PASSED=$(echo "$TEST_OUTPUT" | grep -oP '\d+ passed' | head -1 || echo "?")
    echo "PASS ($PASSED)"
fi

# ── Determine file set for scans 4-8 ──
MODIFIED=$(git diff --name-only HEAD 2>/dev/null | grep '\.rs$' || true)
RS_FILES=()
if [ "$DO_FULL" = true ]; then
    mapfile -t RS_FILES < <(find src/ -name '*.rs' 2>/dev/null || true)
    echo "  Scanning ALL ${#RS_FILES[@]} Rust file(s) in src/."
elif [ -n "$MODIFIED" ]; then
    for f in $MODIFIED; do
        [ -f "$f" ] && RS_FILES+=("$f")
    done
    echo "  Scanning ${#RS_FILES[@]} modified Rust file(s)."
else
    echo "  No Rust files modified and --full not set."
fi
echo ""

# Cache: track which files have module-level #[cfg(test)]
declare -A CFG_TEST_FILES

# Pre-scan for module-level cfg(test) detection
pre_scan_cfg_test() {
    for file in "${RS_FILES[@]}"; do
        [ -f "$file" ] || continue
        # Check if file contains #[cfg(test)] anywhere (module-level is usually at bottom)
        if grep -q '#\[cfg(test)\]' "$file" 2>/dev/null; then
            CFG_TEST_FILES["$file"]="yes"
        fi
    done
}

# Run pre-scan
pre_scan_cfg_test

# ── Helper: check if line is in test code ──
is_test_code() {
    local file="$1" linenum="$2"

    # Fast path: if file has module-level #[cfg(test)], check if we're inside it
    if [ "${CFG_TEST_FILES[$file]+isset}" = "isset" ]; then
        # Find the line of the first #[cfg(test)] and see if our line is after it
        # and before the matching closing }
        local cfg_line test_start
        cfg_line=$(grep -n '#\[cfg(test)\]' "$file" 2>/dev/null | head -1 | cut -d: -f1)
        if [ -n "$cfg_line" ] && [ "$linenum" -gt "$cfg_line" ]; then
            return 0  # inside cfg(test) module
        fi
    fi

    # Check if the file itself is a test file (name contains _test or tests)
    local basename
    basename=$(basename "$file")
    if echo "$basename" | grep -q '_test\.rs$\|tests\.rs$\|_tests\.rs$'; then
        return 0
    fi

    # Check context: look back up to 200 lines for #[test], fn test_, #[cfg(test)]
    local start=$((linenum - 200))
    [ $start -lt 1 ] && start=1
    local context
    context=$(sed -n "${start},${linenum}p" "$file" 2>/dev/null || true)
    if echo "$context" | grep -qF '#[test]' || \
       echo "$context" | grep -q 'fn test_' || \
       echo "$context" | grep -qF '#[cfg(test)]' || \
       echo "$context" | grep -qF '#[cfg(test'; then
        return 0
    fi
}

# ── Helper: check for M-ALLOW/M-UNWRAP markers ──
has_marker() {
    local file="$1" linenum="$2"
    local start=$((linenum - 3))
    [ $start -lt 1 ] && start=1
    local context
    context=$(sed -n "${start},${linenum}p" "$file" 2>/dev/null || true)
    echo "$context" | grep -q '// M-ALLOW\|// M-UNWRAP'
}

# ── Helper: run a file-level scan, returns 0 if files were available ──
has_files() {
    [ ${#RS_FILES[@]} -gt 0 ]
}

# ── 4. Stub scan ──
echo -n "  [4/8] stub scan (todo!/unimplemented!/panic!)... "
if has_files; then
    STUB_COUNT=0
    STUB_LINES=""
    for file in "${RS_FILES[@]}"; do
        [ -f "$file" ] || continue
        while IFS= read -r line; do
            linenum=$(echo "$line" | cut -d: -f1)
            if ! is_test_code "$file" "$linenum"; then
                STUB_LINES="$STUB_LINES\n    $line"
                STUB_COUNT=$((STUB_COUNT + 1))
            fi
        done < <(grep -n 'todo!\|unimplemented!\|panic!("not yet\|panic!("TODO\|panic!("not implemented' "$file" 2>/dev/null || true)
    done
    if [ $STUB_COUNT -gt 0 ]; then
        echo "FAIL ($STUB_COUNT)"
        add_block "$STUB_COUNT stub(s) in non-test code:$STUB_LINES"
    else
        echo "PASS"
    fi
else
    echo "SKIPPED (no files to scan)"
    add_skip "stub scan -- no files in scope"
fi

# ── 5. unwrap/expect scan ──
echo -n "  [5/8] unwrap/expect scan... "
if has_files; then
    UNWRAP_COUNT=0
    UNWRAP_LINES=""
    for file in "${RS_FILES[@]}"; do
        [ -f "$file" ] || continue
        while IFS= read -r line; do
            linenum=$(echo "$line" | cut -d: -f1)
            if ! is_test_code "$file" "$linenum" && ! has_marker "$file" "$linenum"; then
                UNWRAP_LINES="$UNWRAP_LINES\n    $line"
                UNWRAP_COUNT=$((UNWRAP_COUNT + 1))
            fi
        done < <(grep -n '\.unwrap()\|\.expect(' "$file" 2>/dev/null || true)
    done
    if [ $UNWRAP_COUNT -gt 0 ]; then
        echo "WARN ($UNWRAP_COUNT)"
        add_warn "$UNWRAP_COUNT unwrap()/expect() in non-test code (without M-ALLOW marker):$UNWRAP_LINES"
    else
        echo "PASS"
    fi
else
    echo "SKIPPED (no files to scan)"
    add_skip "unwrap scan -- no files in scope"
fi

# ── 6. dead_code scan (individual + crate/module-level) ──
echo -n "  [6/8] #[allow(dead_code)] scan... "
DEAD_COUNT=0
DEAD_LINES=""

# Individual attributes in scanned files
for file in "${RS_FILES[@]:-}"; do
    [ -f "$file" ] || continue
    while IFS= read -r line; do
        linenum=$(echo "$line" | cut -d: -f1)
        if ! is_test_code "$file" "$linenum"; then
            DEAD_LINES="$DEAD_LINES\n    $line"
            DEAD_COUNT=$((DEAD_COUNT + 1))
        fi
    done < <(grep -n '#\[allow(dead_code)\]' "$file" 2>/dev/null || true)
done

# ALWAYS check for crate/module-level #![allow(dead_code)] — these suppress globally
# regardless of diff/full mode, because they affect the entire crate
while IFS= read -r line; do
    DEAD_LINES="$DEAD_LINES\n    $line"
    DEAD_COUNT=$((DEAD_COUNT + 1))
done < <(grep -rn '#!\[allow(dead_code)\]' src/ 2>/dev/null || true)

if [ $DEAD_COUNT -gt 0 ]; then
    echo "FAIL ($DEAD_COUNT)"
    add_block "$DEAD_COUNT #[allow(dead_code)] found (includes crate/module-level):$DEAD_LINES"
elif [ ${#RS_FILES[@]} -eq 0 ]; then
    # Only skipped if no individual files AND no crate-level found
    echo "SKIPPED (no files to scan)"
    add_skip "dead_code scan -- no files in scope"
else
    echo "PASS"
fi

# ── 7. allow-without-reason ──
echo -n "  [7/8] #[allow(...)] without reason=... "
if has_files; then
    ALLOW_COUNT=0
    ALLOW_LINES=""
    for file in "${RS_FILES[@]}"; do
        [ -f "$file" ] || continue
        while IFS= read -r line; do
            linenum=$(echo "$line" | cut -d: -f1)
            if ! is_test_code "$file" "$linenum"; then
                # Look ahead up to 5 lines for reason= (handles multi-line attributes)
                has_reason=false
                for offset in 0 1 2 3 4 5; do
                    check_line=$((linenum + offset))
                    if sed -n "${check_line}p" "$file" | grep -q 'reason\s*='; then
                        has_reason=true
                        break
                    fi
                    # Stop at end of attribute (closing bracket on its own line or end of macro)
                    if sed -n "${check_line}p" "$file" | grep -q '^\s*]\s*$'; then
                        break
                    fi
                done
                if [ "$has_reason" = false ]; then
                    ALLOW_LINES="$ALLOW_LINES\n    $line"
                    ALLOW_COUNT=$((ALLOW_COUNT + 1))
                fi
            fi
        done < <(grep -n '#\[allow(' "$file" 2>/dev/null | grep -v '#\[test\]\|clippy::' || true)
    done
    if [ $ALLOW_COUNT -gt 0 ]; then
        echo "WARN ($ALLOW_COUNT)"
        add_warn "$ALLOW_COUNT #[allow(...)] without reason=:$ALLOW_LINES"
    else
        echo "PASS"
    fi
else
    echo "SKIPPED (no files to scan)"
    add_skip "allow-without-reason scan -- no files in scope"
fi

# ── 8. Bad comments ──
echo -n "  [8/8] FIXME/HACK/XXX/TEMPORARY scan... "
if has_files; then
    BAD_COUNT=0
    BAD_LINES=""
    for file in "${RS_FILES[@]}"; do
        [ -f "$file" ] || continue
        while IFS= read -r line; do
            linenum=$(echo "$line" | cut -d: -f1)
            if ! is_test_code "$file" "$linenum"; then
                BAD_LINES="$BAD_LINES\n    $line"
                BAD_COUNT=$((BAD_COUNT + 1))
            fi
        done < <(grep -n 'FIXME\|HACK\|XXX\|TEMPORARY' "$file" 2>/dev/null || true)
    done
    if [ $BAD_COUNT -gt 0 ]; then
        echo "FAIL ($BAD_COUNT)"
        add_block "$BAD_COUNT FIXME/HACK/XXX/TEMPORARY comments:$BAD_LINES"
    else
        echo "PASS"
    fi
else
    echo "SKIPPED (no files to scan)"
    add_skip "bad comments scan -- no files in scope"
fi

# ── Summary ──
echo ""
BLOCK_COUNT=${#BLOCKS[@]}
WARN_COUNT=${#WARNS[@]}

echo "══════════════════════════════════════════════"
echo "  QUALITY GATE: $PROJECT ($([ "$DO_FULL" = true ] && echo "FULL" || echo "DIFF-ONLY"))"
if [ $BLOCK_COUNT -gt 0 ]; then
    echo "  BLOCKED: $BLOCK_COUNT blocking, $WARN_COUNT warnings, $SKIP_COUNT skipped"
    echo ""
    for d in "${BLOCKS[@]}"; do
        echo "  BLOCK: $d"
    done
    for w in "${WARNS[@]}"; do
        echo "  $w"
    done
elif [ $WARN_COUNT -gt 0 ]; then
    echo "  PASSED: $BLOCK_COUNT blocking, $WARN_COUNT warnings, $SKIP_COUNT skipped"
    echo ""
    for w in "${WARNS[@]}"; do
        echo "  $w"
    done
elif [ $SKIP_COUNT -gt 0 ]; then
    echo "  INCOMPLETE: $BLOCK_COUNT blocking, $WARN_COUNT warnings, $SKIP_COUNT skipped"
    echo ""
    echo "  Some scans were skipped (no files in scope)."
    echo "  This means pre-existing debt was NOT checked."
else
    echo "  CLEAN: $BLOCK_COUNT blocking, $WARN_COUNT warnings, $SKIP_COUNT skipped"
fi

if [ "$DO_FULL" = false ]; then
    echo ""
    echo "  NOTE: Diff-only mode. Pre-existing debt not fully checked."
    echo "  Run with --full for complete repo audit."
fi
echo "══════════════════════════════════════════════"

# ── Log ──
if [ "$DO_LOG" = true ]; then
    printf '{"ts":"%s","agent":"%s","project":"%s","blocking":%d,"warnings":%d,"skipped":%d,"mode":"%s","cwd":"%s"}\n' \
        "$TIMESTAMP" "$(whoami)" "$PROJECT" "$BLOCK_COUNT" "$WARN_COUNT" "$SKIP_COUNT" \
        "$([ "$DO_FULL" = true ] && echo "full" || echo "diff")" "$CWD" >> "$LOGFILE" 2>/dev/null
fi

# ── JSON output ──
if [ "$DO_JSON" = true ]; then
    printf '{"ts":"%s","project":"%s","blocking":%d,"warnings":%d,"skipped":%d,"mode":"%s","blocks":%s,"warns":%s}\n' \
        "$TIMESTAMP" "$PROJECT" "$BLOCK_COUNT" "$WARN_COUNT" "$SKIP_COUNT" \
        "$([ "$DO_FULL" = true ] && echo "full" || echo "diff")" \
        "$(printf '%s\n' "${BLOCKS[@]}" | jq -R . | jq -s . 2>/dev/null || echo '[]')" \
        "$(printf '%s\n' "${WARNS[@]}" | jq -R . | jq -s . 2>/dev/null || echo '[]')"
fi

# ── Exit ──
if [ $BLOCK_COUNT -gt 0 ]; then
    exit 1
elif [ $WARN_COUNT -gt 0 ]; then
    exit 2
else
    exit 0
fi
