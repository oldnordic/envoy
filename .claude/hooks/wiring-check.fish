#!/usr/bin/env fish
# Wiring verification hook for envoy - runs after Rust file edits
# Checks for dead modules, proper sqlitegraph integration, debug prints

cd "$CLAUDE_PROJECT_DIR" 2>/dev/null || cd (pwd)

set -l issues 0

# 1. Dead module detection
echo "=== Dead Module Check ==="
if test -f src/lib.rs
    grep "pub mod\|mod " src/lib.rs | while read -l line
        set -l mod (string match -rg '.*(?:pub )?mod ([^;]*);.*' "$line")
        if test -n "$mod"
            set -l usages (grep -rn "$mod":: src/ --include="*.rs" 2>/dev/null | grep -v "$mod.rs:" | grep -v "lib.rs:" | head -1)
            if test -z "$usages"
                echo "WARNING: Module '$mod' may be dead (declared in lib.rs but unused elsewhere)"
                set issues (math $issues + 1)
            end
        end
    end
else
    echo "SKIP: No src/lib.rs yet"
end

# 2. Debug message cleanup
echo ""
echo "=== Debug Message Cleanup ==="
if grep -rn "eprintln!\|dbg!" src/ --include="*.rs" 2>/dev/null
    echo "WARNING: Found debug print statements"
    set issues (math $issues + 1)
else
    echo "OK: No debug print statements found"
end

# 3. Unwrap/expect check (Result<T> preferred)
echo ""
echo "=== Unwrap/Expect Check ==="
set -l unwrap_count (grep -rn "\.unwrap()\|\.expect(" src/ --include="*.rs" 2>/dev/null | grep -v "//.*M-ALLOW\|//.*M-UNWRAP\|#\[cfg(test)\]" | wc -l)
if test "$unwrap_count" -gt 0
    echo "WARNING: Found $unwrap_count .unwrap()/.expect() calls (mark with M-ALLOW or M-UNWRAP if intentional)"
    set issues (math $issues + 1)
else
    echo "OK: No bare unwrap/expect calls"
end

# 4. sqlitegraph dependency check
echo ""
echo "=== sqlitegraph Integration Check ==="
if test -f Cargo.toml
    if grep -q "sqlitegraph" Cargo.toml
        echo "OK: sqlitegraph dependency declared"
    else
        echo "WARNING: envoy depends on sqlitegraph but it's not in Cargo.toml"
        set issues (math $issues + 1)
    end
else
    echo "SKIP: No Cargo.toml yet"
end

echo ""
if test $issues -gt 0
    echo "FAILED: $issues wiring issue(s) found"
    exit 1
else
    echo "PASSED: All wiring checks passed"
    exit 0
end
