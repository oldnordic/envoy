#!/usr/bin/env fish
# build-check.fish - Pre-commit build verification
# Exit code 2 blocks commit if build fails

cd "$CLAUDE_PROJECT_DIR" 2>/dev/null || cd (pwd)

echo "  Running build verification..."

# Cargo check
echo "    cargo check..."
set -l CHECK_OUTPUT (cargo check --all-features 2>&1 | tail -10)
set -l CHECK_STATUS $status
if test $CHECK_STATUS -ne 0
    echo "❌ cargo check FAILED"
    echo "$CHECK_OUTPUT"
    exit 2
end

# Cargo test
echo "    cargo test..."
set -l TEST_OUTPUT (cargo test --all-features 2>&1 | tail -15)
set -l TEST_STATUS $status
if test $TEST_STATUS -ne 0
    echo "❌ cargo test FAILED"
    echo "$TEST_OUTPUT"
    exit 2
end

# Cargo clippy
echo "    cargo clippy..."
set -l CLIPPY_OUTPUT (cargo clippy --all-targets --all-features 2>&1 || true)
set -l CLIPPY_ERRORS (echo "$CLIPPY_OUTPUT" | grep -cE '^error' || true)
if test "$CLIPPY_ERRORS" -gt 0
    echo "❌ clippy found $CLIPPY_ERRORS error(s)"
    echo "$CLIPPY_OUTPUT" | grep -E '^error' | head -5
    exit 2
end

echo "  ✓ Build verification passed"
exit 0