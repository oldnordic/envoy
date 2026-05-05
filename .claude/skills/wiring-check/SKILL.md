---
name: wiring-check
description: Verify that all code changes are properly wired into the envoy project. Checks for dead modules, sqlitegraph integration, proper error handling, and debug cleanup.
---

# wiring-check

Run a comprehensive wiring verification across the envoy project.

## Usage

```
/wiring-check
```

## Checks Performed

### 1. Dead Module Detection
- Scans `src/lib.rs` for `pub mod` declarations
- Verifies each module is imported or used somewhere in `src/`
- Reports any module that is only used in its own tests

### 2. sqlitegraph Integration
- Verifies sqlitegraph is declared in Cargo.toml
- Checks that pub/sub API is properly used (Event, Channel, Subscription types)

### 3. Error Handling
- Ensures Result<T> is used over bare unwrap/expect
- Checks for proper error propagation in the event pipeline
- Flags any `panic!` in non-test code

### 4. Debug Message Cleanup
- Ensures no `eprintln!` or `dbg!` macros exist in `src/`
- Ensures no debug feature flags are left active

### 5. Event Schema Compliance
- Verifies event payload includes required fields: status, working_on, waiting_for, magellan_trace
- Checks that Channel and Event types match the schema design

## Commands Used

```bash
# Dead modules
grep -n "pub mod\|mod " src/lib.rs | while read line; do mod=$(echo $line | sed 's/.*mod \([^;]*\);.*/\1/'); grep -rn "${mod}::" src/ --include="*.rs" | grep -v "${mod}.rs:" | grep -v "lib.rs:" | head -1 || echo "DEAD: $mod"; done

# Debug messages
grep -rn "eprintln!\|dbg!" src/

# Unwrap/expect
grep -rn "\.unwrap()\|\.expect(" src/ --include="*.rs" | grep -v "M-ALLOW\|M-UNWRAP"

# sqlitegraph dependency
grep "sqlitegraph" Cargo.toml
```

## Exit Codes
- `0` — All wiring checks passed
- `1` — One or more wiring issues found (see report)
