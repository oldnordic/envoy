#!/usr/bin/env fish
# ci-check-hermes.fish — Hermes subagent_stop hook for CI verification
# Checks GitHub CI status after a subagent that pushed code.
# Returns JSON context to parent if CI failed.
#
# Source: ~/.hermes/hooks/hermes/ci-check-hermes.fish

set -l PAYLOAD (cat)
set -l CWD (echo "$PAYLOAD" | python3 -c "import json,sys; d=json.load(sys.stdin); print(d.get('cwd',''))" 2>/dev/null)
set -l CHILD_STATUS (echo "$PAYLOAD" | python3 -c "import json,sys; d=json.load(sys.stdin); print(d.get('extra',{}).get('child_status',''))" 2>/dev/null)

# Only check on completed subagents
if test "$CHILD_STATUS" != "completed"
    exit 0
end

# Find project root
set -l PROJECT_DIR "$CWD"
set -l FOUND (git -C "$PROJECT_DIR" rev-parse --show-toplevel 2>/dev/null)
if test -n "$FOUND"
    set PROJECT_DIR "$FOUND"
end

cd "$PROJECT_DIR" 2>/dev/null
set -l PROJECT_NAME (basename (pwd))

# Check if gh CLI is available and authenticated
if not command -v gh >/dev/null 2>&1
    exit 0
end
if not gh auth status >/dev/null 2>&1
    exit 0
end

# Get GitHub repo
set -l REPO (git remote get-url origin 2>/dev/null | sed 's|git@github.com:||;s|https://github.com/||;s|\.git$||')
if test -z "$REPO"
    exit 0
end

set -l BRANCH (git branch --show-current 2>/dev/null)
if test -z "$BRANCH"
    exit 0
end

# Check for recent pushes (within 15 minutes)
set -l LAST_PUSH (git log --oneline -1 --format="%ci" origin/$BRANCH 2>/dev/null)
if test -n "$LAST_PUSH"
    set -l PUSH_TS (date -d "$LAST_PUSH" +%s 2>/dev/null)
    set -l NOW (date +%s)
    set -l AGE (math $NOW - $PUSH_TS 2>/dev/null)
    if test -n "$AGE" -a $AGE -gt 900
        exit 0
    end
end

# Get latest CI run
set -l RUNS (gh run list --repo "$REPO" --branch "$BRANCH" --limit 3 --json databaseId,status,conclusion,name,headSha,createdAt 2>/dev/null)
if test -z "$RUNS"
    exit 0
end

set -l LATEST_RUN (echo "$RUNS" | python3 -c "
import json, sys
runs = json.load(sys.stdin)
if runs:
    print(json.dumps(runs[0]))
" 2>/dev/null)

if test -z "$LATEST_RUN"
    exit 0
end

set -l RUN_ID (echo "$LATEST_RUN" | python3 -c "import json,sys; print(json.load(sys.stdin)['databaseId'])" 2>/dev/null)
set -l RUN_STATUS (echo "$LATEST_RUN" | python3 -c "import json,sys; print(json.load(sys.stdin)['status'])" 2>/dev/null)
set -l RUN_CONCLUSION (echo "$LATEST_RUN" | python3 -c "import json,sys; d=json.load(sys.stdin); print(d.get('conclusion',''))" 2>/dev/null)
set -l RUN_NAME (echo "$LATEST_RUN" | python3 -c "import json,sys; print(json.load(sys.stdin)['name'])" 2>/dev/null)

if test "$RUN_STATUS" = "queued" -o "$RUN_STATUS" = "in_progress"
    set -l MSG "⚠️ CI STILL RUNNING for $PROJECT_NAME ($RUN_NAME #$RUN_ID, status: $RUN_STATUS).\n\nThe subagent pushed code but CI has not completed. You MUST:\n1. Wait: gh run watch $RUN_ID --repo $REPO\n2. Check results: gh run view $RUN_ID --repo $REPO\n3. If it fails, fix and push again\n4. Do NOT report the task as complete until CI is green."
    echo (echo "$MSG" | python3 -c "import json,sys; print(json.dumps({'context': sys.stdin.read()}))")
    exit 0
end

if test "$RUN_CONCLUSION" = "failure"
    set -l FAILED_LOGS (gh run view "$RUN_ID" --repo "$REPO" --log-failed 2>&1 | tail -40)
    set -l MSG "❌ CI FAILED for $PROJECT_NAME ($RUN_NAME #$RUN_ID).\n\n$FAILED_LOGS\n\nTHE SUBAGENT PUSHED CODE THAT BROKE CI. You MUST:\n1. Classify: fmt | clippy | compile | test | terminology | license | claims | other\n2. Fix in source code\n3. Verify: cargo fmt && cargo check && cargo test\n4. Push: git commit -m 'fix(ci): <description>' && git push\n5. Wait for green CI before reporting completion\n6. If ambiguous — open GitHub issue, do NOT guess"
    echo (echo "$MSG" | python3 -c "import json,sys; print(json.dumps({'context': sys.stdin.read()}))")
    exit 0
end

# CI passed or cancelled — no context injection needed
exit 0
