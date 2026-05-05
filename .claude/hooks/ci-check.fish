#!/usr/bin/env fish
# ci-check.fish — CI self-heal hook (SubagentStop)
# Forces Claude to check GitHub CI status after pushing.
# Exit code 2 blocks the subagent from stopping if CI is failing.
#
# Flow:
#   1. Check if there are unpushed commits or recent pushes
#   2. Use `gh run list` to find the latest CI run on current branch
#   3. If in_progress/queued: block and tell Claude to wait + check again
#   4. If failed: block and provide failure details for Claude to fix
#   5. If success: pass
#
# Source: ~/.hermes/hooks/claude/ci-check.fish
# Deployed to: magellan, llmgrep, mirage, splice, sqlitegraph

cd "$CLAUDE_PROJECT_DIR" 2>/dev/null || cd (pwd)
set -l PROJECT_NAME (basename (pwd))

# Check if gh CLI is available
if not command -v gh >/dev/null 2>&1
    echo "  ℹ️  gh CLI not available (skipping CI check)"
    exit 0
end

# Check if gh is authenticated
if not gh auth status >/dev/null 2>&1
    echo "  ℹ️  gh CLI not authenticated (skipping CI check)"
    exit 0
end

# Get the GitHub repo from git remote
set -l REPO (git remote get-url origin 2>/dev/null | sed 's|git@github.com:||;s|https://github.com/||;s|\.git$||')
if test -z "$REPO"
    echo "  ℹ️  No GitHub remote found (skipping CI check)"
    exit 0
end

# Get current branch
set -l BRANCH (git branch --show-current 2>/dev/null)
if test -z "$BRANCH"
    echo "  ℹ️  Not on a branch (skipping CI check)"
    exit 0
end

# Check if there were recent pushes (within last 15 minutes)
set -l LAST_PUSH (git log --oneline -1 --format="%ci" origin/$BRANCH 2>/dev/null)
set -l NOW (date +%s)
if test -n "$LAST_PUSH"
    set -l PUSH_TS (date -d "$LAST_PUSH" +%s 2>/dev/null)
    set -l AGE (math $NOW - $PUSH_TS 2>/dev/null)
    # If last push was more than 30 minutes ago, skip CI check
    if test -n "$AGE" -a $AGE -gt 1800
        echo "  ℹ️  Last push was >30 min ago (skipping CI check)"
        exit 0
    end
end

# Fetch latest CI runs
set -l RUNS (gh run list --repo "$REPO" --branch "$BRANCH" --limit 3 --json databaseId,status,conclusion,name,headSha,createdAt 2>/dev/null)
if test -z "$RUNS"
    echo "  ℹ️  No CI runs found (skipping CI check)"
    exit 0
end

# Get the most recent run
set -l LATEST_RUN (echo "$RUNS" | python3 -c "
import json, sys
runs = json.load(sys.stdin)
if runs:
    print(json.dumps(runs[0]))
else:
    print('')
" 2>/dev/null)

if test -z "$LATEST_RUN"
    echo "  ℹ️  No CI runs found (skipping CI check)"
    exit 0
end

set -l RUN_ID (echo "$LATEST_RUN" | python3 -c "import json,sys; print(json.load(sys.stdin)['databaseId'])" 2>/dev/null)
set -l RUN_STATUS (echo "$LATEST_RUN" | python3 -c "import json,sys; print(json.load(sys.stdin)['status'])" 2>/dev/null)
set -l RUN_CONCLUSION (echo "$LATEST_RUN" | python3 -c "import json,sys; d=json.load(sys.stdin); print(d.get('conclusion',''))" 2>/dev/null)
set -l RUN_SHA (echo "$LATEST_RUN" | python3 -c "import json,sys; print(json.load(sys.stdin)['headSha'])" 2>/dev/null)
set -l RUN_NAME (echo "$LATEST_RUN" | python3 -c "import json,sys; print(json.load(sys.stdin)['name'])" 2>/dev/null)

# Get current HEAD SHA
set -l HEAD_SHA (git rev-parse HEAD 2>/dev/null)

echo ""
echo "══════════════════════════════════════════════"
echo "  🔁 CI STATUS CHECK — $PROJECT_NAME"
echo "══════════════════════════════════════════════"
echo ""
echo "  Run: $RUN_NAME #$RUN_ID"
echo "  Status: $RUN_STATUS"
if test -n "$RUN_CONCLUSION"
    echo "  Conclusion: $RUN_CONCLUSION"
end
echo "  Branch: $BRANCH"
echo ""

# Handle different states
if test "$RUN_STATUS" = "queued" -o "$RUN_STATUS" = "in_progress"
    echo "  ⏳ CI is still running ($RUN_STATUS)"
    echo ""
    echo "  You MUST wait for CI to complete before finishing."
    echo ""
    echo "  Wait by running:"
    echo "    gh run watch $RUN_ID --repo $REPO"
    echo ""
    echo "  Then check results:"
    echo "    gh run view $RUN_ID --repo $REPO"
    echo ""
    echo "  If CI fails, read the failure logs and fix:"
    echo "    gh run view $RUN_ID --repo $REPO --log-failed"
    echo ""
    echo "  Classify the failure (fmt, clippy, compile, test, terminology, license, claims, other)"
    echo "  and fix accordingly. Push the fix and verify CI passes."
    echo ""
    exit 2

else if test "$RUN_CONCLUSION" = "failure"
    echo "  ❌ CI FAILED"
    echo ""
    echo "  Getting failure logs..."
    echo ""

    # Get the failed step logs
    set -l FAILED_LOGS (gh run view "$RUN_ID" --repo "$REPO" --log-failed 2>&1)
    set -l LOG_STATUS $status

    if test $LOG_STATUS -eq 0 -a -n "$FAILED_LOGS"
        echo "$FAILED_LOGS" | tail -80
    else
        echo "  Could not fetch logs. Try manually:"
        echo "    gh run view $RUN_ID --repo $REPO --log-failed"
    end

    echo ""
    echo "  ═══════════════════════════════════════════"
    echo "  CI FAILED — YOU MUST FIX THIS BEFORE STOPPING"
    echo "  ═══════════════════════════════════════════"
    echo ""
    echo "  Steps:"
    echo "  1. Read the failure logs above carefully"
    echo "  2. Classify: fmt | clippy | compile | test | terminology | license | claims | other"
    echo "  3. Fix the issue in source code:"
    echo "     fmt          → cargo fmt"
    echo "     clippy       → fix the lint (prefer real fix over #[allow])"
    echo "     compile      → fix type errors, missing imports"
    echo "     test         → fix the failing assertion"
    echo "     terminology  → remove AI/LLM terms from public docs"
    echo "     license      → fix GPL-3.0-or-later → GPL-3.0"
    echo "     claims       → remove 'production-ready'"
    echo "     other        → investigate, may need to open issue"
    echo "  4. Run: cargo fmt && cargo check && cargo test"
    echo "  5. git add -u && git commit -m 'fix(ci): <description>' && git push"
    echo "  6. Wait for CI: gh run watch --repo $REPO"
    echo ""
    echo "  If the failure is ambiguous or requires architecture changes,"
    echo "  open a GitHub issue instead of guessing:"
    echo "    gh issue create --repo $REPO --title 'CI failure: <description>' --body '<diagnosis>'"
    echo ""
    exit 2

else if test "$RUN_CONCLUSION" = "success"
    # Check if HEAD matches the CI run's SHA (not a stale success)
    if test "$HEAD_SHA" = "$RUN_SHA"
        echo "  ✅ CI PASSED (SHA matches HEAD)"
        echo ""
        exit 0
    else
        echo "  ⚠️  CI passed but for a different commit (stale)"
        echo "     CI SHA:  $RUN_SHA"
        echo "     HEAD:    $HEAD_SHA"
        echo ""
        echo "  Your latest push may not have triggered CI yet."
        echo "  Check: gh run list --repo $REPO --branch $BRANCH --limit 1"
        echo ""
        exit 0
    end

else if test "$RUN_CONCLUSION" = "cancelled"
    echo "  ⚠️  CI was cancelled (not blocking)"
    echo ""
    exit 0

else
    echo "  ℹ️  CI status: $RUN_STATUS / $RUN_CONCLUSION (not blocking)"
    echo ""
    exit 0
end
