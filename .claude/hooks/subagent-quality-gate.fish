#!/usr/bin/env fish
# subagent-quality-gate.fish — Hermes subagent_stop hook
# Catches subagent lies: stubs, placeholders, todo!, fixme, incomplete work.
# Returns JSON context to parent so it knows the child failed.
#
# Key insight: only checks FILES MODIFIED by the subagent (git diff --name-only),
# not the entire codebase. This catches new violations without flagging historical ones.
#
# Input: stdin JSON from Hermes with hook_event_name, cwd, extra
# Output: stdout JSON {"context": "..."} if violations found
#
# Source: ~/.hermes/hooks/hermes/subagent-quality-gate.fish

# Read payload from stdin
set -l PAYLOAD (cat)
set -l CWD (echo "$PAYLOAD" | python3 -c "import json,sys; d=json.load(sys.stdin); print(d.get('cwd',''))" 2>/dev/null)
set -l CHILD_STATUS (echo "$PAYLOAD" | python3 -c "import json,sys; d=json.load(sys.stdin); print(d.get('extra',{}).get('child_status',''))" 2>/dev/null)

# Only check on completed subagents
if test "$CHILD_STATUS" != "completed"
    exit 0
end

# Find project root (look for Cargo.toml)
set -l PROJECT_DIR "$CWD"
if test -f "$PROJECT_DIR/Cargo.toml"
    # already there
else
    set -l FOUND (git -C "$PROJECT_DIR" rev-parse --show-toplevel 2>/dev/null)
    if test -n "$FOUND"
        set PROJECT_DIR "$FOUND"
    end
end

if not test -f "$PROJECT_DIR/Cargo.toml"
    exit 0
end

cd "$PROJECT_DIR"
set -l PROJECT_NAME (basename "$PROJECT_DIR")

# Get list of Rust files modified vs HEAD (what the subagent actually touched)
set -l MODIFIED (git diff --name-only HEAD -- '*.rs' 2>/dev/null)

if test -z "$MODIFIED"
    # No Rust files changed — nothing to check
    exit 0
end

set -l VIOLATIONS ""
set -l BLOCK_COUNT 0
set -l WARN_COUNT 0
set -l FILE_COUNT 0

for file in $MODIFIED
    if not test -f "$file"
        continue
    end
    set FILE_COUNT (math $FILE_COUNT + 1)
    set -l FILE_VIOLATIONS ""

    # ── 1. todo!() / unimplemented!() / panic!("not yet") ──
    # Only flag in non-test functions (not within #[test] or fn test_ or mod tests)
    set -l STUBS (grep -n 'todo!\|unimplemented!\|panic!("not yet\|panic!("TODO\|panic!("not implemented' "$file" 2>/dev/null || true)
    if test -n "$STUBS"
        # Filter: skip lines inside test modules/functions
        set -l REAL_STUBS ""
        for entry in $STUBS
            set -l linenum (echo "$entry" | cut -d: -f1)
            # Look back up to 5 lines for test markers
            set -l START (math $linenum - 5 2>/dev/null; or echo 1)
            if test $START -lt 1; set START 1; end
            set -l CONTEXT (sed -n "{$START},{$linenum}p" "$file" 2>/dev/null)
            if not echo "$CONTEXT" | grep -q '#\[test\]\|fn test_\|#\[cfg(test)\]\|#\[cfg(test'
                set REAL_STUBS "$REAL_STUBS$entry\n"
            end
        end
        if test -n "$REAL_STUBS"
            set BLOCK_COUNT (math $BLOCK_COUNT + 1)
            set FILE_VIOLATIONS "$FILE_VIOLATIONS\n  STUB: $REAL_STUBS"
        end
    end

    # ── 2. FIXME / HACK / XXX comments (not TODO — that's normal) ──
    set -l BAD_COMMENTS (grep -n 'FIXME\|HACK\|XXX\|TEMPORARY' "$file" 2>/dev/null || true)
    if test -n "$BAD_COMMENTS"
        set BLOCK_COUNT (math $BLOCK_COUNT + 1)
        set FILE_VIOLATIONS "$FILE_VIOLATIONS\n  CRITICAL COMMENT:\n$BAD_COMMENTS"
    end

    # ── 3. "for now" / "placeholder" / "stub implementation" in non-test code ──
    set -l LAZY (grep -ni 'stub implement\|dummy implement\|for now$\|will implement later\|fill.*later\|not yet implemented\|todo: implement' "$file" 2>/dev/null || true)
    if test -n "$LAZY"
        # Filter test code
        set -l REAL_LAZY ""
        for entry in $LAZY
            set -l linenum (echo "$entry" | cut -d: -f1)
            set -l START (math $linenum - 5 2>/dev/null; or echo 1)
            if test $START -lt 1; set START 1; end
            set -l CONTEXT (sed -n "{$START},{$linenum}p" "$file" 2>/dev/null)
            if not echo "$CONTEXT" | grep -q '#\[test\]\|fn test_\|#\[cfg(test)'
                set REAL_LAZY "$REAL_LAZY$entry\n"
            end
        end
        if test -n "$REAL_LAZY"
            set BLOCK_COUNT (math $BLOCK_COUNT + 1)
            set FILE_VIOLATIONS "$FILE_VIOLATIONS\n  LAZY PLACEHOLDER:\n$REAL_LAZY"
        end
    end

    # ── 4. #[allow(dead_code)] — forbidden ──
    set -l DEAD (grep -n '#\[allow(dead_code)\]' "$file" 2>/dev/null || true)
    if test -n "$DEAD"
        set BLOCK_COUNT (math $BLOCK_COUNT + 1)
        set FILE_VIOLATIONS "$FILE_VIOLATIONS\n  dead_code ALLOW:\n$DEAD"
    end

    # ── 5. unwrap()/expect() in non-test code ──
    set -l UNWRAPS (grep -n '\.unwrap()\|\.expect(' "$file" 2>/dev/null || true)
    if test -n "$UNWRAPS"
        # Filter: skip test code and M-ALLOW/M-UNWRAP markers
        set -l REAL_UNWRAPS ""
        for entry in $UNWRAPS
            set -l linenum (echo "$entry" | cut -d: -f1)
            set -l START (math $linenum - 3 2>/dev/null; or echo 1)
            if test $START -lt 1; set START 1; end
            set -l CONTEXT (sed -n "{$START},{$linenum}p" "$file" 2>/dev/null)
            if not echo "$CONTEXT" | grep -q '#\[test\]\|fn test_\|#\[cfg(test)\]\|// M-ALLOW\|// M-UNWRAP'
                set REAL_UNWRAPS "$REAL_UNWRAPS$entry\n"
            end
        end
        if test -n "$REAL_UNWRAPS"
            set WARN_COUNT (math $WARN_COUNT + 1)
            set -l UCOUNT (echo "$REAL_UNWRAPS" | grep -c '.' 2>/dev/null)
            set FILE_VIOLATIONS "$FILE_VIOLATIONS\n  UNWRAP/EXPECT ($UCOUNT in non-test):"
            set FILE_VIOLATIONS "$FILE_VIOLATIONS\n"(echo "$REAL_UNWRAPS" | head -5)
        end
    end

    # ── 6. #[allow(...)] without reason= ──
    set -l NO_REASON (grep -n '#\[allow(' "$file" 2>/dev/null | grep -v 'reason\s*=\|dead_code\|#\[test\]\|clippy::' || true)
    if test -n "$NO_REASON"
        set WARN_COUNT (math $WARN_COUNT + 1)
        set FILE_VIOLATIONS "$FILE_VIOLATIONS\n  ALLOW WITHOUT REASON:\n$NO_REASON"
    end

    if test -n "$FILE_VIOLATIONS"
        set VIOLATIONS "$VIOLATIONS\n[$file]$FILE_VIOLATIONS"
    end
end

# ── 7. cargo fmt (only if .rs files were modified) ──
set -l FMT_OUT (cargo fmt --check 2>&1)
if test $status -ne 0
    set BLOCK_COUNT (math $BLOCK_COUNT + 1)
    set VIOLATIONS "$VIOLATIONS\n[CARGO FMT] Code is not formatted. Run: cargo fmt"
end

# ── 8. cargo check (only if .rs files were modified) ──
set -l CHECK_OUT (cargo check --quiet 2>&1)
if test $status -ne 0
    set BLOCK_COUNT (math $BLOCK_COUNT + 1)
    set -l ERRORS (echo "$CHECK_OUT" | head -20)
    set VIOLATIONS "$VIOLATIONS\n[CARGO CHECK] Compilation failed:\n$ERRORS"
end

# ── OUTPUT ──
if test $BLOCK_COUNT -gt 0 -o $WARN_COUNT -gt 0
    set -l SEVERITY "BLOCKING"
    if test $BLOCK_COUNT -eq 0
        set SEVERITY "WARNING"
    end

    set -l MSG ""
    set MSG "$MSG\nSUBAGENT QUALITY GATE: $SEVERITY ($PROJECT_NAME)"
    set MSG "$MSG\nFiles checked: $FILE_COUNT | Blocking: $BLOCK_COUNT | Warnings: $WARN_COUNT"
    set MSG "$MSG\n$VIOLATIONS"

    if test $BLOCK_COUNT -gt 0
        set MSG "$MSG\n\nTHE SUBAGENT REPORTED SUCCESS BUT LEFT BLOCKING VIOLATIONS."
        set MSG "$MSG\nDO NOT trust the subagent summary. You MUST:"
        set MSG "$MSG\n1. Read each violation above — check the actual file, do not trust the subagent"
        set MSG "$MSG\n2. Fix: remove stubs, implement missing code, remove dead_code, run cargo fmt"
        set MSG "$MSG\n3. Verify: cargo fmt && cargo check && cargo test"
        set MSG "$MSG\n4. Confirm the fix is real implementation, not another stub/placeholder"
        set MSG "$MSG\n5. If the subagent ran out of context, resume the work yourself"
    else
        set MSG "$MSG\n\nWarnings found — review recommended but not blocking."
    end

    # Output as JSON context for parent agent
    echo (echo "$MSG" | python3 -c "import json,sys; print(json.dumps({'context': sys.stdin.read()}))")
end

exit 0
