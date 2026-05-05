---
name: pre-commit-verify
description: Pre-commit verification for envoy — ensures compilation, tests, formatting, and stub checks pass before allowing commits.
---

# Envoy Pre-Commit Verification

Run this skill before committing any changes to the envoy codebase.

## Purpose

Ensure all critical checks pass before allowing a commit. This prevents broken code from entering the repository.

## Verification Steps

Execute the following checks in order. STOP and report failures immediately.

### 1. Format Check

```bash
cargo fmt --check
```

### 2. Compilation Check

```bash
cargo check
```

### 3. Tests

```bash
cargo test
```

### 4. Stub Check

```bash
# No panic!/todo!/unimplemented! in non-test code
grep -rn 'panic!\|todo!\|unimplemented!' src/ --include='*.rs' | grep -v '#\[test\]' | grep -v 'fn test_'
```

### 5. Quality Gate

```bash
.claude/scripts/quality-gate.sh --full
```

### 6. Graph Update (if magellan available)

```bash
if test -f .magellan/envoy.db
    magellan watch --root . --db .magellan/envoy.db --scan-initial &
    sleep 2
    kill %1 2>/dev/null
end
```

## Rules

- **Report each result** as PASS or FAIL with evidence
- **Do NOT claim success** without running the actual commands
- **If any check fails**, stop and report the failure before proceeding
- **No TODOs or stubs** allowed in committed code
