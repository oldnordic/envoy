#!/usr/bin/env fish
# envoy-wrapper.fish — Runs a hook, captures result, posts to envoy, then exits
# Usage: envoy-wrapper.fish <hook_path> [args...]
#
# Design: transparent wrapper. Runs the hook, captures stdout + exit code,
# posts to envoy (fire-and-forget), re-emits stdout, exits with original code.

set -l HOOK_PATH "$argv[1]"
set -l HOOK_ARGS $argv[2..-1]

if test -z "$HOOK_PATH"; or not test -f "$HOOK_PATH"
    exit 1
end

set -l HOOK_NAME (basename "$HOOK_PATH" .fish)
set -l ENVOY_URL "$ENVOY_URL"
test -z "$ENVOY_URL"; and set ENVOY_URL "http://127.0.0.1:9876"

# Run the hook, capture output and exit code
set -l OUTPUT (fish "$HOOK_PATH" $HOOK_ARGS 2>&1)
set -l EXIT_CODE $status

# Re-emit the output so the caller sees it
for line in $OUTPUT
    echo $line
end

# Post to envoy (fire-and-forget, 2s timeout, background)
set -l PROJECT_NAME (basename (pwd))
set -l TRUNCATED (echo "$OUTPUT" | tail -c 500 | string replace -a '"' '\"' | string replace -a '\n' '\\n' | tr '\n' ' ')
set -l PAYLOAD "{\"project\":\"$PROJECT_NAME\",\"hook_name\":\"$HOOK_NAME\",\"exit_code\":$EXIT_CODE,\"output\":\"$TRUNCATED\"}"

curl -s -m 2 -X POST "$ENVOY_URL/events/hook" \
    -H 'Content-Type: application/json' \
    -d "$PAYLOAD" >/dev/null 2>&1 &

exit $EXIT_CODE
