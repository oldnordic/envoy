#!/usr/bin/env fish
# query-schema-check.fish - Pre-code schema verification
# Runs on SubagentStart. Exit code 2 blocks the subagent from starting.
#
# PURPOSE: Ensure database is healthy BEFORE any code changes.
# This prevents LLMs from coding against corrupted/stale schema.

cd "$CLAUDE_PROJECT_DIR" 2>/dev/null || cd (pwd)

echo "══════════════════════════════════════════════"
echo "  🔍 SCHEMA VERIFICATION (Pre-Code Check)"
echo "══════════════════════════════════════════════"
echo ""

# Step 1: Check if magellan DB exists
echo "  [1/4] Checking database existence..."
if not test -f .magellan/envoy.db
    echo ""
    echo "  ❌ ERROR: Magellan database not found at .magellan/envoy.db"
    echo ""
    echo "  You MUST index the project before coding:"
    echo "  → magellan index --db .magellan/envoy.db ."
    echo ""
    echo "  This ensures you're coding against ground truth,"
    echo "  not hallucinated symbols or invented schema."
    echo ""
    exit 2
end
echo "  ✓ Database exists"

# Step 2: Check database health with magellan status
echo ""
echo "  [2/4] Running magellan status..."
set -l STATUS_OUTPUT (magellan status --db .magellan/envoy.db 2>&1)
set -l STATUS $status

if test $STATUS -ne 0
    echo ""
    echo "  ❌ ERROR: magellan status failed"
    echo ""
    echo "$STATUS_OUTPUT" | head -20
    echo ""
    echo "  Database may be corrupted. Try:"
    echo "  → magellan doctor --db .magellan/envoy.db"
    echo "  → If needed: rm .magellan/envoy.db && magellan index --db .magellan/envoy.db ."
    echo ""
    exit 2
end

# Extract key metrics (improved parsing)
set -l FILES_COUNT (echo "$STATUS_OUTPUT" | grep "files:" | grep -o '[0-9]*' | head -1)
set -l SYMBOLS_COUNT (echo "$STATUS_OUTPUT" | grep "symbols:" | grep -o '[0-9]*' | head -1)
set -l CALLS_COUNT (echo "$STATUS_OUTPUT" | grep "calls:" | grep -o '[0-9]*' | head -1)

# Default to "unknown" if parsing failed
if test -z "$FILES_COUNT"
    set FILES_COUNT "unknown"
end
if test -z "$SYMBOLS_COUNT"
    set SYMBOLS_COUNT "unknown"
end
if test -z "$CALLS_COUNT"
    set CALLS_COUNT "unknown"
end

echo "  ✓ Database healthy"
echo "    Files: $FILES_COUNT | Symbols: $SYMBOLS_COUNT | Calls: $CALLS_COUNT"

# Step 3: Run magellan doctor for schema drift detection
echo ""
echo "  [3/4] Running magellan doctor..."
set -l DOCTOR_OUTPUT (magellan doctor --db .magellan/envoy.db 2>&1)
set -l DOCTOR $status

if test $DOCTOR -ne 0
    echo ""
    echo "  ⚠️  WARNING: magellan doctor found issues"
    echo ""
    echo "$DOCTOR_OUTPUT" | head -30
    echo ""
    echo "  Schema drift detected. This may cause:"
    echo "  - Queries returning wrong results"
    echo "  - Symbols not found"
    echo "  - Code generation based on stale schema"
    echo ""
    echo "  Recommended: Run magellan doctor fixes before coding."
    echo "  Proceeding with caution (not blocking, but risky)."
    # Don't block on doctor warnings, but make them visible
else
    echo "  ✓ No schema drift detected"
end

# Step 4: Check for concurrent access (file lock)
echo ""
echo "  [4/4] Checking for concurrent access..."
if test -f .magellan/envoy.db-shm
    # WAL mode shared memory file exists - check if it's stale
    set -l SHM_AGE (math (date +%s) - (stat -c %Y .magellan/envoy.db-shm 2>/dev/null || echo (date +%s)))
    
    if test $SHM_AGE -gt 3600
        echo "  ⚠️  WARNING: Stale WAL shared memory file (>1 hour old)"
        echo "  Consider: rm .magellan/envoy.db-shm .magellan/envoy.db-wal"
    else
        echo "  ✓ WAL mode active (normal)"
    end
else
    echo "  ✓ No concurrent access detected"
end

echo ""
echo "══════════════════════════════════════════════"
echo "  ✅ SCHEMA CHECK PASSED"
echo "══════════════════════════════════════════════"
echo ""
echo "  You are cleared to code against ground truth."
echo "  Remember: Query BEFORE writing code."
echo "  → magellan find --db .magellan/envoy.db --name \"similar\""
echo "  → llmgrep search --db .magellan/envoy.db --query \"pattern\""
echo ""

exit 0
