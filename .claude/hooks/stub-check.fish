#!/usr/bin/env fish
# stub-check.fish — Canonical stub detection hook (SubagentStop)
# Catches panic!/todo!/unimplemented! in non-test Rust code.
# Exit code 2 blocks the subagent from stopping.
#
# Source: ~/.hermes/hooks/claude/stub-check.fish
# Deployed to: magellan, llmgrep, mirage, splice, sqlitegraph

cd "$CLAUDE_PROJECT_DIR" 2>/dev/null || cd (pwd)
set -l PROJECT_NAME (basename (pwd))

echo "  Checking for stub code in $PROJECT_NAME..."

# Only check if Rust files were modified
set -l MODIFIED (git diff --name-only HEAD 2>/dev/null)
if test -z "$MODIFIED"; or not string match -qr '\.rs$' -- "$MODIFIED"
    echo "  ✓ No Rust files modified (skipping)"
    exit 0
end

# Find all panic!/todo!/unimplemented! occurrences
set -l ALL_PANICS (grep -rn 'panic!\|todo!\|unimplemented!' src/ --include='*.rs' 2>/dev/null || true)

if test -z "$ALL_PANICS"
    echo "  ✓ No panic!/todo!/unimplemented! statements found"
    exit 0
end

# Filter out test code (lines within 5 lines of #[test] or fn test_)
set -l NON_TEST_PANICS
for line in $ALL_PANICS
    set -l file (string split ':' $line)[1]
    set -l linenum (string split ':' $line)[2]

    # Check if this is test code (look for #[test] or fn test_ within 5 lines before)
    set -l context_start (math $linenum - 5)
    if test $context_start -lt 1
        set context_start 1
    end

    set -l context (sed -n "$context_start,$linenum"p "$file" 2>/dev/null || true)
    if not string match -qr '#\[test\]|fn test_' -- "$context"
        set -a NON_TEST_PANICS $line
    end
end

if test (count $NON_TEST_PANICS) -gt 0
    echo ""
    echo "  ❌ FAILED: Found "(count $NON_TEST_PANICS)" stub(s) in NON-TEST code:"
    echo ""
    for line in $NON_TEST_PANICS
        echo "    $line"
    end
    echo ""
    echo "  These MUST be resolved before completing:"
    echo "  - panic!() → proper error handling with Result<T>"
    echo "  - todo!()  → implement the feature or remove"
    echo "  - unimplemented!() → implement or return error"
    echo ""
    exit 2
end

echo "  ✓ No stubs found (panic!/todo! in tests is allowed)"
exit 0
