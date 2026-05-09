# Envoy-Atheneum Integration Test Report

**Date:** 2026-05-09  
**Version:** 0.3.0  
**Method:** Grounded-coding skill Phase 0 analysis

## Summary

Full integration testing was performed using grounded-coding methodology (graph analysis, source code inspection, live endpoint testing). 5 issues were documented.

## Documentation Created

### 1. API Documentation
**File:** `docs/atheneum-api.md` (new)

Comprehensive API documentation including:
- All 6 endpoints with request/response examples
- Configuration (environment variables, systemd)
- Error handling
- Discovery types and handoff status values
- **Correct port 9876 documented throughout**

### 2. Grounded-Coding Skill Updates
**File:** `~/.claude/skills/grounded-coding/SKILL.md`

Changes made:
- Fixed all `localhost:8080` → `127.0.0.1:9876` (9 occurrences)
- Updated atheneum availability check to use knowledge query instead of non-existent health field

## Issues Documented

### Issue #1: Port Mismatch (FIXED)
**Status:** ✅ Fixed in skill  
**Location:** `~/.claude/skills/grounded-coding/SKILL.md`  
**Problem:** Skill referenced port 8080, envoy runs on 9876  
**Fix:** All references updated to `127.0.0.1:9876`

### Issue #2: Health Check Missing Atheneum Status (DOCUMENTED)
**Status:** ⚠️ Documented, not fixed (per "no hotfixes" instruction)  
**Location:** `src/http.rs` — `health` handler, `HealthResponse` struct  
**Problem:** Health endpoint doesn't indicate atheneum availability  
**Current Response:**
```json
{"status": "ok", "uptime_seconds": 2386, "agents_online": 0}
```
**Workaround in skill:** Use `/atheneum/knowledge?target=__health_check__` to detect availability

### Issue #3: Discoveries Endpoint Requires Target Parameter (DOCUMENTED)
**Status:** ⚠️ By design  
**Problem:** No way to list all discoveries without specifying target  
**Impact:** Low - API design choice

### Issue #4: Token Savings Always Returns 0 (DOCUMENTED)
**Status:** ⚠️ Requires investigation  
**Location:** `GET /atheneum/knowledge` response  
**Problem:** Token savings calculation not working or not triggered  
**Impact:** Low - metric not critical for functionality

### Issue #5: Handoff Manifest Structure (DOCUMENTED)
**Status:** ⚠️ Documented in API docs  
**Problem:** Request expects nested `manifest` field, may be unclear  
**Impact:** MEDIUM - addressed in API documentation with examples

## Test Results

| Component | Status | Evidence |
|-----------|--------|----------|
| HTTP Endpoints (6/6) | ✅ Working | All return expected responses |
| Data Persistence | ✅ Working | Atheneum.db (122KB) growing |
| MCP Tools (4) | ✅ Defined | envoy-channel.js lines 405-444, 608-670 |
| API Documentation | ✅ Created | docs/atheneum-api.md |
| Skill Port References | ✅ Fixed | All 9 occurrences updated |
| Skill Atheneum Check | ✅ Fixed | Uses knowledge query now |

## Evidence Sources

### Graph Analysis
```bash
magellan status --db .magellan/envoy.db  # 13 files, 295 symbols indexed
magellan find --db .magellan/envoy.db --name "store_discovery"  # Found at line 272
magellan find --db .magellan/envoy.db --name "get_knowledge"    # Found at line 443
```

### Source Code Read
- `src/http.rs` lines 200-480: Handler implementations
- `src/http.rs` lines 640-750: Request/response type definitions
- `src/main.rs`: Port configuration (9876 default)
- `.config/systemd/user/envoy.service`: Environment variables

### Live Testing
```bash
curl -X POST http://127.0.0.1:9876/atheneum/discoveries  # ✅ 201 Created
curl http://127.0.0.1:9876/atheneum/discoveries?target=X  # ✅ 200 OK
curl http://127.0.0.1:9876/health  # ✅ Returns status
```

## Remaining Work (Not Hotfixed)

Per user instruction "no hotfixes", these are documented for future work:

1. **Add `atheneum_configured` to health endpoint** — Would require modifying `HealthResponse` struct and handler
2. **Implement token savings calculation** — Requires atheneum library changes
3. **Add "list all discoveries" endpoint** — New endpoint, requires design discussion

## Files Changed

**Envoy repo:**
- `docs/atheneum-api.md` (new) — 358 lines

**Skill file:**
- `~/.claude/skills/grounded-coding/SKILL.md` — Port fixes, atheneum check update
