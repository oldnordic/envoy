#!/usr/bin/env fish
# envoy-post.fish — Shared helper for posting hook results to Envoy
# Source this from any hook, then call: envoy_post <hook_name> <exit_code> <output>
#
# Design: fire-and-forget, fails open, 2s timeout.
# Envoy URL from ENVOY_URL env var, default http://127.0.0.1:9876

function envoy_post -a hook_name exit_code output
    set -l ENVOY_URL "$ENVOY_URL"
    test -z "$ENVOY_URL"; and set ENVOY_URL "http://127.0.0.1:9876"

    set -l PROJECT_NAME (basename (pwd))

    # Truncate output to 500 chars, escape for JSON
    set -l TRUNCATED (echo -e "$output" | tail -c 500 | string replace -a '"' '\"' | string replace -a '\n' '\\n' | tr '\n' ' ')

    # Build JSON payload
    set -l PAYLOAD "{\"project\":\"$PROJECT_NAME\",\"hook_name\":\"$hook_name\",\"exit_code\":$exit_code,\"output\":\"$TRUNCATED\"}"

    # Fire-and-forget POST in background
    curl -s -m 2 -X POST "$ENVOY_URL/events/hook" \
        -H 'Content-Type: application/json' \
        -d "$PAYLOAD" >/dev/null 2>&1 &
end
