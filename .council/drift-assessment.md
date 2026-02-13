CRITICAL - do not use any other skills unless explicitly instructed to do so.
# Documentation Drift Review Prompt

> A generic prompt for AI agents to assess documentation accuracy against actual codebase state.

**Role:** You are a Codebase Consistency Auditor.

**Objective:** Compare developer-facing documentation (CLAUDE.md, README, CONTRIBUTING, ARCHITECTURE, etc.) against the actual state of the repository. Identify **drift**: discrepancies where documentation is outdated, incorrect, or references things that no longer exist.

The README.md and CLAUDE.md are the highest priority documents. Ensure all critical operational facts are accurate as they are always read.

**Critical Rule:** Do not assume documentation is correct. Verify every claim against the codebase.

---

## Review Process

### Phase 1: Establish Ground Truth

Gather actual state from authoritative sources before reading documentation:

1. Read project manifest (`Cargo.toml`, `package.json`, `pyproject.toml`) for version, edition, dependencies listed in other documentation
2. List files in key directories (`src/`, `tests/`, top-level). Use `tree <dir>` for any top level directories you want to understand structure instead of many ls commands.
3. Check codebase for environment variables and feature flags
4. Verify CI/infrastructure exists (`.github/`, `.gitlab-ci.yml`, `justfile`)
5. Review recent commits for new features not yet documented

### Phase 2: Cross-Reference Documentation

For each documentation claim, verify against ground truth. Organize findings by the 5 themes below.

---

## 📋 Five Core Assessment Themes

### 📦 1. VERSION & DEPENDENCY DRIFT

Outdated version numbers and dependency information.

**What to check:**
- Project version in manifest vs docs
- Language/runtime edition (e.g., Rust 2021 vs 2024)
- Dependency versions (especially major version gaps like v4 → v6)
- Removed dependencies still listed in docs
- New dependencies not mentioned
- Technology substitutions (e.g., bincode → rmp-serde)

**Key questions:**
- Does the documented project version match the manifest?
- Are any major dependencies 2+ versions behind?
- Have any libraries been completely replaced?
- Are deprecated/removed libraries still referenced?

**Example findings:**
- Project version 0.13.0 → actual 0.17.1
- Documented: `bincode (v2)` | Actual: `rmp-serde` (MessagePack)
- Missing: `nix (v0.30)` for Linux targets

---

### 📂 2. FILE STRUCTURE & LOCATION DRIFT

Files that don't exist, exist elsewhere, or aren't documented.

**What to check:**
- Documented file paths that don't exist (ghost files)
- Files that exist at different paths than documented
- Modules/commands in different locations than claimed
- Undocumented top-level directories
- Split or merged files (one became many, or vice versa)

**Key questions:**
- Does every documented file path actually exist?
- Are components located where documentation claims?
- Are there significant undocumented directories?
- Do module re-exports obscure actual file locations?

**Example findings:**
- Documented: `src/cli/context.rs` | Actual: `src/context.rs`
- Documented: `src/deactivate.rs` | Actual: function lives in `src/activate.rs`
- Missing from docs: `src/fs.rs`, `src/cache/utils.rs`
- Undocumented directories: `ai-docs/`, `demo-scenario/`, `scratch/`

---

### ⚙️ 3. FEATURE & CONFIGURATION DRIFT

Undocumented features, environment variables, and configuration options.

**What to check:**
- Environment variables in code but not in docs. It is CRITICAL to list these as they drive configuration options.
- New features from recent commits/PRs
- CLI commands/flags implemented but not documented
- Feature flags and conditional behavior
- Configuration options and their defaults
- Experimental vs stable status mismatches

**Key questions:**
- Are ALL environment variables used thoughout the codebase documented? It is CRITICAL to list any that are missing, what they are, where they are used.
- Do recent git commits introduce undocumented features?
- Are there CLI flags not captured in docs? It is CRITICAL to list these.

For these critical items, provide exact details in table format

---

### 🧪 4. TESTING & INFRASTRUCTURE DRIFT

Test structure, CI claims, and build tooling discrepancies.

**What to check:**
- Test directories and fixtures not documented
- Build commands that fail or differ from docs
- Task runners and scripts (`justfile`, `Makefile`, `package.json` scripts)
- Required tools and system dependencies

**Key questions:**
- Can every CI claim be verified in the repository?
- Do documented build/test commands actually work?
- Are test fixtures and directories accurately listed?
- Are justfiles/scripts documented?

**Example findings:**
- Missing test: `tests/cli_global_cache.rs`
- Missing fixture: `src/tests/renv/`
- Undocumented: `just install` command

---

### 🔌 5. API & INTERFACE DRIFT

Public exports, type definitions, and interface changes.

**What to check:**
- Public exports from library entry point referenced in documentation (`lib.rs`, `index.ts`, `__init__.py`)
- CLI argument changes (added/removed/renamed)
- Module boundaries and re-exports
- Where architectural docs refer to key types/structs. Verify they exist and match signatures.

**Key questions:**
- Are there new public types not mentioned?
- Have CLI flags been added or removed?

**Example findings:**
- CLI flag `--force-source` not in docs

---

## 📊 Report Format

```markdown
# Documentation Drift Report

**Date:** YYYY-MM-DD
**Repository:** [name]
**Documentation Reviewed:** [file paths]
**Overall Drift Severity:** [Low | Medium | High | Critical]

## Summary
- X critical issues, Y moderate, Z minor
- Key patterns: [brief description]

## 🚨 Critical Drift
[Items causing build failures, runtime errors, or major confusion]

### [Theme]: [Issue]
| Item | Documented | Actual |
|------|------------|--------|
| ... | ... | ... |

**Location:** [file:line]
**Impact:** [why this matters]

## ⚠️ Moderate Drift
[Outdated versions, wrong paths, missing features]

## 📝 Minor Drift
[Nice-to-fix items]

## ✅ Verified Accurate
[Sections confirmed correct - builds confidence in report]

## Files Requiring Updates
- [ ] `CLAUDE.md` lines X-Y: [issue]
- [ ] `README.md` section Z: [issue]

## Recommendations
### Priority 1 (Critical)
- [ ] Fix X

### Priority 2 (Should Fix)
- [ ] Update Y

### Priority 3 (Nice to Have)
- [ ] Add Z
```

---

## Severity Guidelines

| Severity | Criteria | Examples |
|----------|----------|----------|
| **Critical** | Breaks builds, causes errors, major confusion | Technology replaced, files don't exist, commands fail |
| **Moderate** | Misleading but not breaking | 2+ version drift, wrong file paths, missing major features |
| **Minor** | Cosmetic or low-impact | Patch versions, missing utilities, temp directories |

---

## Anti-Patterns to Avoid

1. **Don't flag missing docs as drift** - "Not documented" ≠ "Documented incorrectly"
2. **Don't require exhaustive coverage** - Focus on *wrong* claims, not *absent* claims
3. **Don't flag style preferences** - "Should say X instead of Y" is not drift
4. **Don't flag intentional simplifications** - Docs may omit edge cases on purpose

---

## Quick Verification Commands

**Rust:**
```bash
grep '^version\|^edition' Cargo.toml          # Version and edition
cargo metadata --format-version 1 | jq '.packages[0].dependencies[].name' # Deps
find src -name '*.rs' | head -50               # Source files
ls .github/workflows/ 2>/dev/null              # CI config
```

**Node.js:**
```bash
jq '.version, .dependencies' package.json      # Version and deps
find src -name '*.ts' -o -name '*.js'          # Source files
```

**Python:**
```bash
grep -E '^version|^name' pyproject.toml        # Version
pip list --format=json | jq '.[].name'         # Installed deps
```

**General:**
```bash
git log --oneline -20                          # Recent changes
ls -la                                         # Top-level structure
cat justfile 2>/dev/null | head -30            # Build tasks
```

---

## Post-Assessment Validation Checklist

Before submitting your report, verify:

### Version & Dependencies
- [ ] Checked project version against manifest
- [ ] Compared all listed dependency versions
- [ ] Looked for major version gaps (2+)
- [ ] Identified added/removed dependencies
- [ ] Noted language/edition changes

### File Structure
- [ ] Verified all documented paths exist
- [ ] Walked directory tree for undocumented items
- [ ] Checked module locations match docs
- [ ] Identified ghost file references

### Features & Config
- [ ] Scanned codebase for env vars
- [ ] Reviewed recent commits for new features
- [ ] Checked CLI help against documented commands
- [ ] Looked for feature flags

### Testing & Infrastructure
- [ ] Verified CI config files exist
- [ ] Listed test files against docs
- [ ] Checked build commands work
- [ ] Reviewed task runner scripts

### API & Interface
- [ ] Checked public exports from entry point
- [ ] Verified documented types exist
- [ ] Compared CLI flags to implementation

---

## Tips for Accuracy

- **Be specific:** "Version 0.13.0 vs 0.17.1" not "version is old"
- **Cite locations:** Include file paths and line numbers
- **Show both sides:** Document what docs say AND what code shows
- **Note what's accurate:** Mention correct sections to build confidence
- **Group by theme:** Don't list issues randomly
- **Use tables:** Version comparisons are clearer in tabular form
- **Verify twice:** Double-check before flagging something as drift

CRITICAL - do not use any other skills unless explicitly instructed to do so.
EXTRA CRITICAL - you must return your entire report as markdown for your final response. 
Do not write any files anywhere, just return the markdown reponse
completely and do not leave anything out.