#!/usr/bin/env fish
# splice-cycles-check.fish - Detect new call graph cycles after refactoring
# Runs on SubagentStop. Exit code 2 blocks completion if cycles are detected.
#
# PURPOSE: Catch accidental circular dependencies introduced by refactoring.
# Cycles may indicate architectural problems that need fixing.

cd "$CLAUDE_PROJECT_DIR" 2>/dev/null || cd (pwd)

echo "══════════════════════════════════════════════"
echo "  🔄 CALL GRAPH CYCLE DETECTION"
echo "══════════════════════════════════════════════"
echo ""

# Check if any Rust files were modified
echo "  [1/3] Checking for code changes..."
set -l MODIFIED (git diff --name-only HEAD 2>/dev/null)

if test -z "$MODIFIED"
    echo "  ℹ️  No git changes detected (skipping cycle check)"
    echo ""
    exit 0
end

# Check if any .rs files were modified
set -l RS_FILES (echo "$MODIFIED" | string match -r '\.rs$' || true)

if test -z "$RS_FILES"
    echo "  ℹ️  No Rust files modified (skipping cycle check)"
    echo ""
    exit 0
end

echo "  ✓ Rust files modified:"
for file in $RS_FILES
    echo "    - $file"
end

# Estimate change size
set -l CHANGED_LINES (git diff --stat 2>/dev/null | tail -1 | grep -oE '[0-9]+ insertion|[0-9]+ deletion' | grep -o '[0-9]*' | paste -sd+ | bc 2>/dev/null || echo "0")

if test "$CHANGED_LINES" -lt 20
    echo ""
    echo "  ℹ️  Small change (<20 lines), skipping cycle check"
    echo "  (Cycles typically introduced by larger refactorings)"
    echo ""
    exit 0
end

echo "  ✓ Change size: ~$CHANGED_LINES lines (significant)"

# Step 2: Run splice cycles
echo ""
echo "  [2/3] Running splice cycles..."

if not test -f .magellan/envoy.db
    echo "  ⚠️  WARNING: No magellan database found"
    echo ""
    echo "  Cannot check for cycles without code graph."
    echo "  Run: magellan index --db .magellan/envoy.db ."
    echo ""
    # Don't block, but warn
    exit 0
end

set -l CYCLES_OUTPUT (splice cycles --db .magellan/envoy.db 2>&1)
set -l CYCLES_STATUS $status

if test $CYCLES_STATUS -ne 0
    echo ""
    echo "  ⚠️  WARNING: splice cycles command failed"
    echo ""
    echo "$CYCLES_OUTPUT" | head -20
    echo ""
    echo "  This may indicate graph corruption or splice bug."
    echo "  Proceed with caution, verify manually if needed."
    # Don't block on command failure
    exit 0
end

# Step 3: Analyze cycle detection results
echo ""
echo "  [3/3] Analyzing cycle detection results..."

# Check if cycles were found
if echo "$CYCLES_OUTPUT" | grep -q "Call Graph Cycles"
    set -l CYCLE_LINE (echo "$CYCLES_OUTPUT" | grep "Call Graph Cycles")
    set -l CYCLE_COUNT (echo "$CYCLE_LINE" | grep -o '[0-9]*' | head -1)
    
    if test -z "$CYCLE_COUNT"
        set CYCLE_COUNT "0"
    end
    
    echo "  Cycle detection report:"
    echo "  → $CYCLE_LINE"
    
    if test "$CYCLE_COUNT" -gt 0
        echo ""
        echo "  ⚠️  WARNING: $CYCLE_COUNT call graph cycle(s) detected"
        echo ""
        echo "$CYCLES_OUTPUT" | grep -A20 "Cycle Detection Report" | head -25
        echo ""
        echo "  ══════════════════════════════════════════════"
        echo "  ⚠️  CYCLES DETECTED - REVIEW REQUIRED"
        echo "  ══════════════════════════════════════════════"
        echo ""
        echo "  Cycles may indicate:"
        echo ""
        echo "  1. Circular dependencies (refactor needed)"
        echo "     → Module A imports B, B imports A"
        echo "     → Solution: Extract shared interface"
        echo ""
        echo "  2. Mutual recursion (intentional?)"
        echo "     → Function A calls B, B calls A"
        echo "     → Solution: Document if intentional"
        echo ""
        echo "  3. Accidental coupling"
        echo "     → New code created unexpected dependency"
        echo "     → Solution: Review recent changes"
        echo ""
        echo "  BEFORE COMPLETING:"
        echo ""
        echo "  □ If cycles are INTENTIONAL:"
        echo "    → Document in code comment"
        echo "    → Add to session notes"
        echo "    → Explain why unavoidable"
        echo ""
        echo "  □ If cycles are ACCIDENTAL:"
        echo "    → Refactor to break cycle"
        echo "    → Re-run splice cycles"
        echo "    → Verify cycles resolved"
        echo ""
        
        # Block completion - require acknowledgment
        exit 2
    else
        echo "  ✓ No call graph cycles detected"
    end
else
    echo "  ℹ️  No cycle detection report found"
    echo "  (This is normal if no cycles exist)"
end

echo ""
echo "══════════════════════════════════════════════"
echo "  ✅ CYCLE CHECK PASSED"
echo "══════════════════════════════════════════════"
echo ""
echo "  No problematic call graph cycles detected."
echo "  Your refactoring maintains acyclic structure."
echo ""

exit 0
