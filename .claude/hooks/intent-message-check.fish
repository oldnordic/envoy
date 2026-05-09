#!/usr/bin/env fish
# intent-message-check.fish — Pre-work intent enforcement (pre-commit)
# If >5 files changed in a shared repo, requires commit message to contain:
#   Intent: [what] on [repo/file]
#
# Scope: Mandatory for all repos under /home/feanor/Projects/
# Enforcement: Pre-commit hook checks git commit message for "Intent:" line.
#
# Source: ~/.hermes/hooks/intent-message-check.fish
# Deployed to: magellan, llmgrep, mirage, splice, sqlitegraph, envoy

cd "$CLAUDE_PROJECT_DIR" 2>/dev/null || cd (pwd)
set -l PROJECT_NAME (basename (pwd))

# Only enforce for shared repos under /home/feanor/Projects/
set -l PROJECT_ROOT (pwd)
if not string match -q '/home/feanor/Projects/*' "$PROJECT_ROOT"
    echo "  ✓ Not a shared repo (skipping intent check)"
    exit 0
end

echo "  Checking pre-work intent message..."

# Count files changed in this commit
set -l FILES_CHANGED (git diff --cached --name-only 2>/dev/null | wc -l)
if test "$FILES_CHANGED" -le 5
    echo "  ✓ $FILES_CHANGED files changed ≤ 5 (skipping)"
    exit 0
end

# Get the commit message (for pre-commit, we check what's staged)
# Note: pre-commit hook runs before the message is finalized, so we check
# if there are already >5 staged files and warn that the commit message
# should include an Intent line. For actual enforcement, this integrates
# with prepare-commit-msg or commit-msg hook.
set -l COMMIT_MSG_FILE "$argv[1]"
if test -n "$COMMIT_MSG_FILE"; and test -f "$COMMIT_MSG_FILE"
    set -l COMMIT_MSG (cat "$COMMIT_MSG_FILE" 2>/dev/null || true)
    if string match -qi "*Intent:*" "$COMMIT_MSG"
        echo "  ✓ Commit message includes Intent line"
        exit 0
    end
end

# For prepare-commit-msg / commit-msg integration
# Also check if the commit is being amended or merged
if test -f "$COMMIT_MSG_FILE"
    set -l COMMIT_MSG (cat "$COMMIT_MSG_FILE" 2>/dev/null || true)
    # Skip for merge commits, squash, amend
    if string match -qi "*merge*" "$COMMIT_MSG"; or string match -qi "*squash*" "$COMMIT_MSG"
        echo "  ✓ Merge/squash commit (skipping)"
        exit 0
    end
end

echo ""
echo "  ⚠️  WARNING: $FILES_CHANGED files changed in shared repo $PROJECT_NAME"
echo ""
echo "  Pre-work intent message is REQUIRED for >5 file changes:"
echo ""
echo "    Intent: [what] on [repo/file]"
echo ""
echo "  Full format (recommended):"
echo "    Starting: [what]"
echo "    On: [repo/file]"
echo "    Expected: [outcome]"
echo "    Blockers: [any]"
echo ""
echo "  (Advisory during transition — update your commit message to include Intent:)"
echo ""

# Advisory only during transition — will become blocking after adoption period
exit 0
