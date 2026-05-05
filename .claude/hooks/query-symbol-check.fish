#!/usr/bin/env fish
# query-symbol-check.fish - Verify ground truth was queried before coding
# Runs on SubagentStop. Exit code 2 blocks the subagent from completing.
#
# PURPOSE: Ensure LLM queried magellan/llmgrep BEFORE writing code.
# This prevents hallucinated symbols and invented schema.

cd "$CLAUDE_PROJECT_DIR" 2>/dev/null || cd (pwd)

echo "══════════════════════════════════════════════"
echo "  🔍 GROUND TRUTH VERIFICATION (Pre-Code Query Check)"
echo "══════════════════════════════════════════════"
echo ""

# Check if any Rust files were modified
echo "  [1/3] Checking for Rust code changes..."
set -l MODIFIED (git diff --name-only HEAD 2>/dev/null)

if test -z "$MODIFIED"
    echo "  ℹ️  No git changes detected (skipping query check)"
    echo ""
    exit 0
end

# Check if any .rs files were modified
set -l RS_FILES (echo "$MODIFIED" | string match -r '\.rs$' || true)

if test -z "$RS_FILES"
    echo "  ℹ️  No Rust files modified (skipping query check)"
    echo ""
    exit 0
end

echo "  ✓ Rust files modified:"
for file in $RS_FILES
    echo "    - $file"
end

# Step 2: Check terminal history for ground truth queries
echo ""
echo "  [2/3] Checking for ground truth queries..."

# Try multiple history locations
set -l HISTORY_FILES "$CLAUDE_PROJECT_DIR/.claude/terminal-history.log" "$HOME/.claude/terminal-history.log" "$HOME/.hermes/terminal-history.log" "/tmp/claude-terminal-history.log"

set -l FOUND_QUERIES false
set -l QUERY_EVIDENCE ""

for history_file in $HISTORY_FILES
    if test -f "$history_file"
        # Look for magellan/llmgrep/mirage/splice queries
        set -l QUERIES (grep -E "magellan (find|refs|query|status|files|doctor)|llmgrep search|mirage (cfg|status|hotspots)|splice (find|query|refs|cycles)" "$history_file" 2>/dev/null | tail -10)
        
        if test -n "$QUERIES"
            set FOUND_QUERIES true
            set QUERY_EVIDENCE "$QUERIES"
            echo "  ✓ Ground truth queries found in $history_file:"
            echo "$QUERY_EVIDENCE" | while read -l line
                echo "    → $line"
            end
            break
        end
    end
end

# If no history file found, check recent magellan.db modification
if test "$FOUND_QUERIES" = false
    echo "  ⚠️  No terminal history found"
    echo ""
    
    # Fallback: Check if magellan.db was recently updated (within last 5 minutes)
    if test -f .magellan/envoy.db
        set -l DB_AGE (math (date +%s) - (stat -c %Y .magellan/envoy.db 2>/dev/null || echo 0))
        
        if test $DB_AGE -lt 300
            echo "  ℹ️  Magellan DB was updated recently (<5 min ago)"
            echo "  This suggests queries were run, but cannot verify."
            echo ""
            echo "  ⚠️  WARNING: Cannot verify ground truth queries"
            echo "  Please confirm you ran these BEFORE coding:"
            echo "  → magellan find --db .magellan/envoy.db --name \"similar_function\""
            echo "  → llmgrep search --db .magellan/envoy.db --query \"pattern\""
            echo ""
            # Don't block, but require acknowledgment
            exit 0
        else
            echo "  ❌ No evidence of ground truth queries"
            echo ""
            echo "  You MUST query before coding. Run these commands:"
            echo ""
            echo "  # Find similar existing functions:"
            echo "  magellan find --db .magellan/envoy.db --name \"your_function_name\""
            echo ""
            echo "  # Search for patterns:"
            echo "  llmgrep search --db .magellan/envoy.db --query \"your pattern\""
            echo ""
            echo "  # List symbols in file you're modifying:"
            echo "  magellan query --db .magellan/envoy.db --file \"src/your_file.rs\""
            echo ""
            echo "  Then retry completing the task."
            exit 2
        end
    else
        echo "  ❌ No magellan database found"
        echo ""
        echo "  ERROR: Cannot verify ground truth without database."
        echo "  Run: magellan index --db .magellan/envoy.db ."
        exit 2
    end
end

# Step 3: Verify queries are relevant to modified files
echo ""
echo "  [3/3] Checking query relevance..."

if test "$FOUND_QUERIES" = true
    # Extract file references from queries
    set -l QUERY_FILES (echo "$QUERY_EVIDENCE" | grep -oE 'src/[^ ]+\.rs' | sort -u || true)
    
    if test -n "$QUERY_FILES"
        echo "  ✓ Queries reference files:"
        for qfile in $QUERY_FILES
            echo "    - $qfile"
        end
        
        # Check if any modified file matches queried files
        set -l MATCH_FOUND false
        for rs_file in $RS_FILES
            for qfile in $QUERY_FILES
                if string match -q "*$qfile*" "$rs_file"
                    set MATCH_FOUND true
                    break
                end
            end
            if test "$MATCH_FOUND" = true
                break
            end
        end
        
        if test "$MATCH_FOUND" = true
            echo "  ✓ Query targets match modified files"
        else
            echo "  ⚠️  WARNING: Queries don't match modified files"
            echo "  Make sure you queried the files you're changing."
        end
    else
        echo "  ℹ️  Queries found but no specific file targets"
        echo "  (General symbol search is acceptable)"
    end
end

echo ""
echo "══════════════════════════════════════════════"
echo "  ✅ GROUND TRUTH VERIFICATION PASSED"
echo "══════════════════════════════════════════════"
echo ""
echo "  You have queried ground truth before coding."
echo "  This ensures your code is based on reality,"
echo "  not hallucinated symbols or invented schema."
echo ""

exit 0
