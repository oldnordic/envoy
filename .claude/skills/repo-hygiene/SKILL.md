---
name: repo-hygiene
description: Enforce repository cleanliness before commits AND after pushes. Audits for AI artifacts, generated databases, internal working docs, and build output. MANDATORY w3m visual review of GitHub remote — pattern greps are insufficient and miss novel violations.
---

# repo-hygiene

Run before every commit AND after every push to ensure the repository stays clean and professional.

## Usage

```
/repo-hygiene
```

## Critical Lesson: Why Grep-Based Audits Fail

Pattern greps only catch **known** bad patterns. They miss:
- Novel internal doc names (`THE_VISION.md`, `MIRAGE_FIX_SUMMARY.md`)
- Empty artifact files (`EOF`)
- Agent config files (`GEMINI.md`)
- Misplaced test files (`test_metadata.rs` at repo root)
- Dev scripts (`test_parse.sh` at repo root)
- Files with unconventional naming that don't match regexes

**Real example from 2026-05-04:**
```
API grep said: "CLEAN"
w3m dump revealed: GEMINI.md, EOF, MIRAGE_FIX_SUMMARY.md,
                   THE_VISION.md, ECOSYSTEM_SYNERGY.md,
                   test_metadata.rs, test_parse.sh
```

**Rule:** Grep is a pre-filter. w3m visual review is the mandatory gate.

---

## Two-Phase Audit (Local + Remote)

### Phase 1: Local Worktree (Pre-Commit)

```bash
# 1. Check what would be committed
git status --short

# 2. Check for tracked files that match forbidden patterns
git ls-files | grep -E '2026-.*session.*\.txt|\.planning/|\.codemcp/|\.kimi/|\.magellan/.*\.db|\.codegraph/.*\.db|knowledge\.db|results/|\.geo$|target_storage_verify/|\.cargo-home/|PLAN_.*\.md|.*_COMPLETE\.md|.*_ASSESSMENT\.md|.*_FIX_.*\.md|.*_SUMMARY\.md|THE_VISION\.md|ECOSYSTEM_.*\.md|GEMINI\.md|INVARIANTS\.md|ANALYSIS\.md|EOF$|test_metadata\.rs|test_parse\.sh'

# 3. Check for large files in the index (>1MB)
git rev-list --objects --all | git cat-file --batch-check='%(objecttype) %(objectname) %(objectsize) %(rest)' | awk '$1 == "blob" && $3 > 1048576 {print $3, $4}' | sort -rn | head -20

# 4. Check for untracked files that should be ignored
git status --short | grep '^??' | grep -E '\.db$|\.geo$|session.*\.txt|\.planning|\.codemcp|\.kimi|results/|target/|\.magellan/'
```

### Phase 2: Remote Verification (Post-Push) — MANDATORY

**This is the gate that caught the mirage mess. Do not skip.**

**Step 1: Open w3m on the GitHub repo URL**

```bash
# Replace with actual OWNER/REPO
w3m https://github.com/oldnordic/magellan
```

**Step 2: Read every file name on the root page**

Scroll through the file list. Look at EACH file name and ask:
- Is this a public-facing document? (README.md, CHANGELOG.md, LICENSE — yes)
- Is this an internal working document? (GEMINI.md, *_FIX_*.md, *_SUMMARY.md — NO)
- Is this a generated artifact? (*.db, *.geo, session dumps — NO)
- Is this a test/dev file at root? (test_*.rs, test_*.sh — probably NO)
- Is this an empty artifact? (EOF, SQL — NO)

**Step 3: Check subdirectories that should exist**

Navigate into `.github/workflows/`, `src/`, `tests/`, `docs/`.
- Verify docs/ contains only public documentation
- Verify tests/ contains only test code
- Verify no `.planning/`, `.codemcp/`, `.kimi/`, `.magellan/` dirs exist

**Step 4: Press `q` to quit w3m**

---

## Alternative: Non-Interactive GitHub API Check

When w3m is not available (e.g., in a script):

```bash
OWNER=oldnordic
REPO=magellan
BRANCH=main

# Dump ALL tracked files — read the full list, do not grep
ghtree() {
  gh api "repos/$OWNER/$REPO/git/trees/$BRANCH?recursive=1" \
    --jq '.tree[] | select(.type == "blob") | .path'
}

# Save to file and REVIEW MANUALLY
ghtree > /tmp/repo-files.txt

# Then: cat /tmp/repo-files.txt | less
# Read every line. Look for anything suspicious.
```

---

## What to Remove (Real Examples from Our Repos)

| File | Why | Found In |
|------|-----|----------|
| `GEMINI.md` | Local agent config | mirage |
| `EOF` | Empty artifact file | mirage |
| `MIRAGE_FIX_SUMMARY.md` | Internal working doc | mirage |
| `THE_VISION.md` | Internal working doc | mirage |
| `ECOSYSTEM_SYNERGY.md` | Internal working doc | mirage |
| `test_metadata.rs` | Test file at root | mirage |
| `test_parse.sh` | Dev script at root | mirage |
| `VALIDATION_GATES_FIX.md` | Internal fix doc | splice |
| `ANALYSIS.md` | Internal analysis doc | llmgrep |
| `2026-*-session-*.txt` | AI session dump | magellan |
| `docs/superpowers/` | Internal skills/plans | magellan |
| `tests/fixtures/databases/*.db` | Generated DB fixture | llmgrep |

---

## Standard .gitignore Template

Every owned repo should have this baseline:

```gitignore
# AI assistant artifacts
2026-*-this-session-*.txt
**/.codemcp/
**/.kimi/

# Generated databases
*.db
*.db-shm
*.db-wal
.magellan/
.codegraph/
knowledge.db
*.geo

# Internal planning (not for published repos)
.planning/

# Experiment results
results/
**/results/

# Build artifacts
target/
**/target/
target_storage_verify/
.cargo-home/

# Backups and temp files
*.bak
*.tmp
*.temp

# Internal docs that should never be public
GEMINI.md
INVARIANTS.md
THE_VISION.md
*_FIX_SUMMARY.md
*_FIX_*.md
ANALYSIS.md
ECOSYSTEM_SYNERGY.md
```

---

## Cleanup Procedure

If audit finds tracked cruft:

1. **Remove from tracking** (keep local files):
   ```bash
   git rm -r --cached .planning results 2026-*.txt .magellan/*.db .codegraph/*.db
   git rm --cached GEMINI.md INVARIANTS.md THE_VISION.md *_FIX_SUMMARY.md
   ```

2. **Update `.gitignore`** to prevent re-addition

3. **Commit the cleanup**:
   ```bash
   git add .gitignore
   git commit -m 'clean: remove generated artifacts and internal working docs from tracking'
   ```

4. **For large binaries already in history**: use `git filter-branch` or `git filter-repo`

5. **Push** and verify on GitHub with **w3m** (not just API grep)

---

## Pre-Commit + Post-Push Checklist

Before `git commit`:
- [ ] `git status --short` shows only intended changes
- [ ] No `2026-*` session dump files are staged
- [ ] No `.db`, `.geo`, or generated database files are staged
- [ ] No `results/` or benchmark output is staged
- [ ] No binaries >1MB are staged
- [ ] `.gitignore` covers all generated artifacts in this repo

After `git push`:
- [ ] **Opened w3m on GitHub repo URL and read every file name**
- [ ] No internal working docs visible on remote (`GEMINI.md`, `*_FIX_*.md`, etc.)
- [ ] No generated artifacts visible on remote (`.magellan/`, `*.db`, `*.geo`)
- [ ] No empty artifact files visible on remote (`EOF`, `SQL`)
- [ ] No misplaced test/dev files at root (`test_*.rs`, `test_*.sh`)

---

## Repo-Specific Notes

- **magellan**: `.magellan/` contains generated DBs; `docs/*PLAN*.md` are internal only
- **mirage/splice/llmgrep/sqlitegraph**: Each has its own `.magellan/<project>.db`
- **geometric_db_concept**: `target_storage_verify/` is a non-standard build dir
- **odincode**: `src/.codemcp/codemcp` is a 30MB binary that must never be tracked
- **Memoria**: `results/` contained 200+ experiment JSON files

## Verification

After cleanup, confirm on GitHub:

1. **Open w3m**: `w3m https://github.com/OWNER/REPO`
2. **Read every file name** on the root page
3. Verify no `.planning/`, `results/`, session dumps, or DB files appear
4. Navigate into `docs/` — verify only public docs exist
5. Check the contributors tab is clean (only human + Claude co-author trailers)
6. Confirm GitHub Actions runs are clean
