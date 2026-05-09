#!/usr/bin/env fish
# ack-contract-check.fish — ACK contract enforcement (SubagentStop)
# Scans recently modified .md message files for bare "ACK" without contract.
# Advisory during transition (warns), mandatory after 1 week.
#
# Required ACK formats:
#   ACK — taking now, ETA <time>
#   ACK — reviewing <section>, ETA <time>
#   ACK — blocked on <X>, will update when unblocked
#   ACK — will check at <time/event>
#
# Data source: /home/feanor/Projects/messages/ (not git diff — messages live
# outside project repos). Finds .md files modified in the last 60 seconds.
#
# Source: /home/feanor/Projects/.claude/hooks/ack-contract-check.fish
# Deployed to: magellan, llmgrep, mirage, splice, sqlitegraph, envoy

set -l MSG_DIR "/home/feanor/Projects/messages"

echo "  Checking ACK contracts in message files..."

# Find .md files modified in the last 60 seconds across all message streams
# This catches files written by any agent during the current work cycle
set -l MESSAGE_FILES (find "$MSG_DIR" -name '*.md' -mmin -1 -not -name 'monitor-status*' 2>/dev/null)

if test -z "$MESSAGE_FILES"
    echo "  ✓ No recent message files (skipping)"
    exit 0
end

# Valid ACK contract patterns — ACK must include action + context
set -l VALID_PATTERNS 'taking now' 'blocked on' 'will check at' 'reviewing' 'ETA' 'done' 'shipped' 'tested' 'verified' 'approved' 'complete' 'waiting for' 'ready to'

# Skip files we wrote ourselves (claude_*, claude1_to_*)
set -l PEER_FILES ""
for f in $MESSAGE_FILES
    set -l bn (basename "$f")
    if not string match -q 'claude_*' "$bn"; and not string match -q 'claude1_to_*' "$bn"
        set -a PEER_FILES "$f"
    end
end

if test -z "$PEER_FILES"
    echo "  ✓ No peer message files (skipping)"
    exit 0
end

set -l BARE_ACKS ""
for file in $PEER_FILES
    # Match ACK at start of line or after common prefixes like "From:", "Date:", etc.
    # Only checks lines where ACK appears as a standalone reply/action, not in code or lists.
    set -l ACK_LINES (grep -n -P '^ACK\b|^\s*ACK\b' "$file" 2>/dev/null || true)
    for line in $ACK_LINES
        # Skip lines inside fenced code blocks
        set -l linenum (string split ':' $line)[1]
        if test "$linenum" -gt 0
            set -l before (head -n "$linenum" "$file" | tail -n 20)
            # Count opening vs closing code fences — if odd, we're inside a code block
            set -l open_fences (echo "$before" | grep -c '^\s*```' 2>/dev/null)
            if test -z "$open_fences"; set open_fences 0; end
            set -l fence_mod (math "$open_fences % 2")
            if test "$fence_mod" -ne 0
                continue
            end
        end

        # Skip if it contains any contract pattern
        set -l HAS_CONTRACT 0
        for pattern in $VALID_PATTERNS
            if string match -qi "*$pattern*" "$line"
                set HAS_CONTRACT 1
                break
            end
        end
        # Also skip "ACK contract" meta-references and headers
        if string match -qi "*contract*" "$line"; or string match -qi "*#*ACK*" "$line"
            set HAS_CONTRACT 1
        end
        if test $HAS_CONTRACT -eq 0
            set -a BARE_ACKS "$file: $line"
        end
    end
end

if test (count $BARE_ACKS) -gt 0
    echo ""
    echo "  ⚠️  WARNING: Found "(count $BARE_ACKS)" bare ACK(s) without contract:"
    echo ""
    for line in $BARE_ACKS
        echo "    $line"
    end
    echo ""
    echo "  Every ACK must include one of:"
    echo "    ACK — taking now, ETA <time>"
    echo "    ACK — reviewing <section>, ETA <time>"
    echo "    ACK — blocked on <X>, will update when unblocked"
    echo "    ACK — will check at <time/event>"
    echo ""
    echo "  (Advisory during transition — will block after 2026-05-14)"
    echo ""
    # Advisory only — exit 0
    exit 0
end

echo "  ✓ All ACKs include contract details"
exit 0
