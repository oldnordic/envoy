#!/usr/bin/env fish
# security-check.fish — Canonical security gate hook (SubagentStop)
# Flags NEW security-sensitive patterns in staged changes.
# Exit code 2 blocks on hard failures (unsafe, SQL inject, secrets).
# Exit 0 with warnings for soft issues (unwrap, Command::new).
#
# Source: ~/.hermes/hooks/claude/security-check.fish
# Deployed to: magellan, llmgrep, mirage, splice, sqlitegraph

cd "$CLAUDE_PROJECT_DIR" 2>/dev/null || cd (pwd)
set -l PROJECT_NAME (basename (pwd))

set -l issues 0
set -l warnings 0

# Only run if Rust files were modified
set -l DIFF (git diff HEAD -- '*.rs' 2>/dev/null)
if test -z "$DIFF"
    echo "  ℹ️  No Rust changes (skipping security check)"
    exit 0
end

echo "══════════════════════════════════════════════"
echo "  🛡️  SECURITY CHECK — $PROJECT_NAME"
echo "══════════════════════════════════════════════"
echo ""

# Helper: extract added lines (starts with + but not +++ for file header)
set -l ADDED_LINES (echo "$DIFF" | grep '^+[^+]' | string sub -s 2)

# 1. NEW unsafe blocks/functions
echo "  [1/5] Checking for new unsafe code..."
set -l NEW_UNSAFE (echo "$ADDED_LINES" | grep -E '^\s*unsafe\s+\{|^\s*unsafe\s+fn\s+' || true)
if test -n "$NEW_UNSAFE"
    echo "  ⚠️  Found new unsafe code:"
    for line in $NEW_UNSAFE
        echo "    → $line"
    end
    set issues (math $issues + 1)
else
    echo "  ✓ No new unsafe code"
end

# 2. NEW unwrap / expect in non-test code
echo ""
echo "  [2/5] Checking for new unwrap/expect in non-test code..."
set -l NEW_UNWRAP (echo "$ADDED_LINES" | grep -E '\.unwrap\(\)|\.expect\(' || true)
if test -n "$NEW_UNWRAP"
    set -l FILTERED
    for line in $NEW_UNWRAP
        set -l line_content (string trim -l -- "$line")
        if string match -qr '#\[test\]|fn test_|mod tests|assert_' -- "$line_content"
            continue
        end
        set -a FILTERED $line
    end
    if test (count $FILTERED) -gt 0
        echo "  ⚠️  Found "(count $FILTERED)" new unwrap/expect in non-test code:"
        for line in $FILTERED
            echo "    → $line"
        end
        set warnings (math $warnings + 1)
    else
        echo "  ✓ No new unwrap/expect in non-test code"
    end
else
    echo "  ✓ No new unwrap/expect in non-test code"
end

# 3. NEW SQL injection patterns (string building into SQL)
echo ""
echo "  [3/5] Checking for new SQL injection vectors..."
set -l NEW_SQL_INJ (echo "$ADDED_LINES" | grep -iE 'query\s*\+.*format!|format!.*SELECT|format!.*INSERT|format!.*UPDATE|format!.*DELETE|format!.*CREATE|push_str.*format!' || true)
if test -n "$NEW_SQL_INJ"
    echo "  ⚠️  Found potential SQL injection vector (string formatting into SQL):"
    for line in $NEW_SQL_INJ
        echo "    → $line"
    end
    set issues (math $issues + 1)
else
    echo "  ✓ No new SQL injection vectors"
end

# 4. NEW hardcoded secrets
echo ""
echo "  [4/5] Checking for new hardcoded secrets..."
set -l NEW_SECRETS (echo "$ADDED_LINES" | grep -iE 'api_key\s*[:=].*[\"\'\''][a-zA-Z0-9]{16,}|token\s*[:=].*[\"\'\''][a-zA-Z0-9]{16,}|password\s*[:=].*[\"\'\'']|secret\s*[:=].*[\"\'\''][a-zA-Z0-9]{8,}|bearer\s+[a-zA-Z0-9]{16,}|Authorization:\s+Bearer' || true)
if test -n "$NEW_SECRETS"
    echo "  🚨 FOUND potential hardcoded secret:"
    for line in $NEW_SECRETS
        echo "    → $line"
    end
    set issues (math $issues + 1)
else
    echo "  ✓ No hardcoded secrets detected"
end

# 5. NEW Command::new with user input
echo ""
echo "  [5/5] Checking for new command injection vectors..."
set -l NEW_CMD_INJ (echo "$ADDED_LINES" | grep -E 'Command::new\s*\(' || true)
if test -n "$NEW_CMD_INJ"
    echo "  ⚠️  Found new Command::new (review for unsanitized user input):"
    for line in $NEW_CMD_INJ
        echo "    → $line"
    end
    set warnings (math $warnings + 1)
else
    echo "  ✓ No new Command::new calls"
end

# Summary
echo ""
echo "══════════════════════════════════════════════"
if test $issues -gt 0
    echo "  ❌ SECURITY ISSUES: $issues blocking, $warnings warnings"
    echo ""
    echo "  BLOCKING — must fix before completing:"
    echo "  - unsafe blocks (document safety preconditions)"
    echo "  - SQL injection vectors (use parameterized queries)"
    echo "  - hardcoded secrets (use env vars or config)"
    echo ""
    echo "  WARNINGS — review recommended:"
    echo "  - unwrap/expect in non-test code (use ? operator)"
    echo "  - Command::new usage (verify input is sanitized)"
    echo ""
    exit 2
else if test $warnings -gt 0
    echo "  ⚠️  $warnings security warning(s) — review recommended"
    echo ""
    exit 0
else
    echo "  ✅ SECURITY CHECK PASSED"
    echo ""
    exit 0
end
