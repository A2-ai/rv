# Design: `rv upgrade <pkg>` — selective (partial) upgrade

Repo: `/Users/wescummings/projects/rv`, branch `upgrade_package`.

## User decisions (binding)

1. `ResolveMode` becomes data-carrying: `PartialUpgrade(HashSet<String>)`, dropping `Copy`.
2. Unlock scope: **Only X** (cargo `update -p` style)
3. Valid targets: any package in the lockfile (including transitive deps).
4. Unknown package names: hard error before any resolution/sync (PR #485 spirit).
5. No-op case: succeed; rely on the existing sync/plan output ("Nothing to do"). No dedicated upgrade-outcome reporting.

## 0. Corrections to prior exploration findings

 **Critical architecture point**: a "filtered lockfile view" cannot be a temporary built inside `Context::resolve`. `ResolvedDependency::from_locked_package` (`src/resolver/dependency.rs:68-92`) stores `Cow::Borrowed` references *into the lockfile* (`name`, `dependencies`, `suggests`, `path`), so the `Resolution<'_>` returned from `Context::resolve` (`src/context.rs:227`) borrows the lockfile for the caller's lifetime. A locally-constructed filtered `Lockfile` would be dropped before the `Resolution` is used. Also, `Lockfile.packages` is a **private** field (`src/lockfile.rs:429`).

**Recommended mechanism instead of a cloned/filtered lockfile: an "unlock set" on the `Resolver`.** Pass the *full* lockfile as today, plus an owned `HashSet<String>` of names the resolver must treat as absent; `lockfile_lookup` returns `None` for those names. Semantically identical to a filtered view, zero lifetime impact, no lockfile clone, no new `Lockfile` API. The unlock set is exactly the named target packages.

## 1. `ResolveMode` change and every call site

### 1.1 The enum (`src/context.rs:48-56`)

```rust
/// Mode for dependency resolution
#[derive(Debug, Clone, PartialEq, Default)]   // Copy removed
pub enum ResolveMode {
    #[default]
    Default,
    FullUpgrade,
    /// Upgrade only the named packages; everything else stays pinned to the
    /// lockfile unless the fresh versions' requirements force re-resolution.
    PartialUpgrade(HashSet<String>),
}

impl ResolveMode {
    pub fn is_upgrade(&self) -> bool {
        matches!(self, ResolveMode::FullUpgrade | ResolveMode::PartialUpgrade(_))
    }
}
```

**Threading: pass by reference (`&ResolveMode`)** everywhere. The mode is only read at each layer; only `Context::resolve` clones the set once, into the `Resolver` (owned, so no new lifetimes leak into `Resolution<'d>`).

### 1.2 Every call site touched

| File:line | Change |
|---|---|
| `src/context.rs:206-213` | `load_for_resolve_mode(&mut self, _resolve_mode: &ResolveMode)` (arg already unused) |
| `src/context.rs:227` | `pub fn resolve(&self, resolve_mode: &ResolveMode) -> Resolution<'_>` |
| `src/context.rs:228-231` | match gains arm: `Default \| PartialUpgrade(_) => &self.lockfile`, `FullUpgrade => &None` |
| `src/context.rs:233-241` | after `Resolver::new`, wire unlock set (§2.2) |
| `src/context.rs:257` | `== FullUpgrade` → `resolve_mode.is_upgrade()` — the `from_lockfile` re-annotation **must also run for `PartialUpgrade`** (§4; gates reinstall skipping at `src/sync/handler.rs:410`) |
| `src/cli/resolution.rs:5-9` | `resolve_dependencies(context: &Context, resolve_mode: &ResolveMode, exit_on_failure: bool)` |
| `src/cli/sync.rs:56-70` | `SyncHelper::run(&self, context: &'a Context, resolve_mode: &ResolveMode)`; pass-through at :70 |
| `src/main.rs:572, 587` (Sync) | pass `&resolve_mode` |
| `src/main.rs:722, 743` (Add) | pass `&resolve_mode` |
| `src/main.rs:800, 812` (Remove) | pass `&resolve_mode` |
| `src/main.rs:814-831` (Upgrade) | rewritten — §3 |
| `src/main.rs:840-844, 861` (Plan) | pass `&upgrade` |
| `src/main.rs:871` (Summary) | `&ResolveMode::Default` |
| `src/main.rs:937, 972, 1046` (Tree/Info/Library) | `&ResolveMode::Default` |
| `src/main.rs:1136, 1144` (Run) | pass `&resolve_mode` |

No other consumers (verified via grep; exports at `src/lib.rs:40` and `src/cli/mod.rs:6` unchanged).

## 2. Resolver + Context wiring

### 2.1 Resolver (`src/resolver/mod.rs`)

- Struct (`mod.rs:92-112`): add owned field `unlocked_packages: HashSet<String>`. Initialize empty in `Resolver::new` (`mod.rs:115-134`); add a setter mirroring `show_progress_bar()`:

```rust
pub fn unlock_packages(&mut self, names: HashSet<String>) {
    self.unlocked_packages = names;
}
```

- `lockfile_lookup` (`mod.rs:189-285`): insert at the top, before the `matching_in_lockfile` check:

```rust
// Partial upgrade: treat these packages as absent from the lockfile
if self.unlocked_packages.contains(item.name.as_ref()) {
    return None;
}
```

That's the entire resolver change. Verified consequences:

- **Locked-dependent concern (decision 6), verified in code**: locked `my.pkg` still hits `lockfile_lookup` and enqueues its recorded deps as `QueueItem`s **carrying the recorded version requirements** (`mod.rs:269-279`). Target `X` misses `lockfile_lookup` (skip-set) and lands in `repositories_lookup` (`mod.rs:287-311`), where `find_package(name, item.version_requirement, ...)` (`src/repository.rs:61-101`) selects the newest version *satisfying the requirement*. If `X` was also dequeued earlier without a requirement, dedup (`mod.rs:530-541`) only skips identical requirement sets, so a second constrained lookup may add a second version of `X` to `found`; `Resolution::finalize`'s SAT solve (`src/resolver/result.rs:73-155`) picks one consistent version and GCs the rest. Dependents' constraints bind `X` both during BFS lookup and in the SAT solve.
- **Closed-graph `validate()` never runs on the in-memory view**: `validate()` (`src/lockfile.rs:444-466`) is called only from `load()` (:537) and `save()` (:501). The skip-set never mutates the lockfile. Write-back (`src/cli/sync.rs:112-132`) builds a fresh `Lockfile::from_resolved` from the closed resolved graph and `save()` re-validates.
- `matching_in_lockfile` (computed at `mod.rs:523-526`) is harmless for targets: the skip-set check fires first.

### 2.2 `Context::resolve` (`src/context.rs:227-269`)

```rust
pub fn resolve(&self, resolve_mode: &ResolveMode) -> Resolution<'_> {
    let lockfile = match resolve_mode {
        ResolveMode::Default | ResolveMode::PartialUpgrade(_) => &self.lockfile,
        ResolveMode::FullUpgrade => &None,
    };

    let mut resolver = Resolver::new(/* unchanged 7 args */);

    if let ResolveMode::PartialUpgrade(targets) = resolve_mode {
        resolver.unlock_packages(targets.clone());
    }
    // ... existing resolve ...
    // existing re-annotation block, condition widened:
    if resolve_mode.is_upgrade() && self.lockfile.is_some() { /* from_lockfile re-annotation */ }
}
```

(If `PartialUpgrade` is reached with `self.lockfile == None`, resolution is fully fresh anyway; the CLI hard-errors before that, §3.)

## 3. CLI: argument, handler, upfront validation

### 3.1 Clap (`src/main.rs:117-121`)

```rust
/// Upgrade packages to the latest versions available
Upgrade {
    /// Packages to upgrade (may be transitive dependencies). If omitted,
    /// upgrades everything.
    #[clap(value_parser)]
    packages: Vec<String>,      // NOT required = true (unlike Remove at :108)
    #[clap(long)]
    dry_run: bool,
},
```

Empty vec ⇒ exactly today's behavior (`FullUpgrade`).

### 3.2 Handler (`src/main.rs:814-831`, rewritten)

```rust
Command::Upgrade { packages, dry_run } => {
    let mut context = Context::new(&cli.config_file, RCommandLookup::Strict)...;
    if !log_enabled { context.show_progress_bar(); }

    let resolve_mode = if packages.is_empty() {
        ResolveMode::FullUpgrade
    } else {
        let targets = validate_upgrade_targets(context.lockfile.as_ref(), &packages,
                                               &context.lockfile_path())?;   // hard error, §3.3
        ResolveMode::PartialUpgrade(targets)
    };

    context.load_for_resolve_mode(&resolve_mode)...;
    SyncHelper { dry_run, output_format: Some(output_format), ..Default::default() }
        .run(&context, &resolve_mode)?;
}
```

Validation runs **before** `load_for_resolve_mode` (before any DB download/resolution/sync).

### 3.3 Validation (decision 4)

New file `src/cli/upgrade.rs`, exported from `src/cli/mod.rs`; `anyhow` (matches `src/cli/sync.rs:5`):

```rust
pub fn validate_upgrade_targets(
    lockfile: Option<&Lockfile>,
    packages: &[String],
    lockfile_path: &Path,
) -> anyhow::Result<HashSet<String>>
```

- `lockfile == None` → error. `Context::new` sets `lockfile = None` in three situations (`src/context.rs:126-142`): file missing, `use_lockfile()` false, or R-version/format mismatch (`Lockfile::load` returns `Ok(None)`, `src/lockfile.rs:528-531`). Message:
  `"rv upgrade <pkg> requires a lockfile. No usable lockfile at {path} (missing, disabled in rproject.toml, or ignored due to R version/format mismatch). Run 'rv sync' first, or use 'rv upgrade' without arguments."`
- Dedupe names into a `HashSet`.
- Collect **all** names absent from `lockfile.package_names()` (`src/lockfile.rs:603-609`; case-sensitive) and error once listing them all:
  `"package(s) not found in lockfile: {a, b}. rv upgrade <pkg> can only target packages present in {lockfile_name}."`
  (Nice-to-have: case-insensitive near-match suggestion.)

## 4. Sync-path correctness prerequisites for partial mode

Up-to-date package → no `SyncChange`; `print_grouped_changes` (`src/cli/sync.rs:194-198`) prints **"Nothing to do"** — this existing output is the no-op behavior (decision 5). Two things must hold for partial mode:

1. **`from_lockfile` re-annotation must run for `PartialUpgrade`** (`src/context.rs:257`). Target `X` resolves fresh (`from_lockfile = false`, `dependency.rs:127`); `handler.rs:410` (`if !self.uses_lockfile || dep.from_lockfile`) would otherwise force a pointless reinstall of an unchanged package. `contains_resolved_dep` (`src/lockfile.rs:596-600`, name+version) flips it back — as `FullUpgrade` does today.
2. Lockfile write-back compares against the untouched original `context.lockfile` and only saves on change — correct as-is (another reason not to mutate it).

## 5. Edge cases

| Case | Behavior |
|---|---|
| Target also in config with changed source (`matching_in_lockfile == false`) | Re-resolves fresh anyway (`mod.rs:195-199`); naming it redundant, harmless. |
| Target `Git{branch/tag}` / `Url` (`could_have_changed`, `lockfile.rs:150-156`) | Already refreshed every resolve; naming it is a no-op relative to plain sync. |
| Git dep pinned to commit | Resolves to the same commit; nothing changes. |
| Target unreachable after GC (orphan lockfile entry) | Validation passes; resolution drops it; sync removes it from library and lockfile. |
| Multiple packages | Deduped set; all validated up front. |
| Target locked as `Source::Builtin` (e.g. MASS) | Skip-set → `builtin_lookup` (`mod.rs:579-589`) resolves at installed R's version. True base packages never in lockfiles → unknown-name error. |
| No lockfile / `use_lockfile = false` / ignored lockfile | Hard validation error (§3.3). Bare `rv upgrade` unaffected. |
| `--dry-run` | Unchanged plumbing; lockfile write gated by `!dry_run` (`sync.rs:112`). |
| `rv plan --upgrade` | Untouched (`FullUpgrade`). `plan --upgrade <pkg>` is a trivial follow-up, out of scope. |
| Cascade (fresh X needs newer/new dep) | Existing per-item unlock (`mod.rs:211-215`) + new deps not in lockfile; `finalize` GC removes deps only old X needed. |
| Unsolvable after upgrade | SAT keeps old-version X candidate when enqueued via dependent's requirement (success — X simply stays put); genuinely unsolvable → `req_failures`, existing failure output, lockfile untouched. |

## 6. Test plan

No pending `.snap.new` files exist — nothing to clean up first.

1. **Resolver snapshot tests (primary).** The `resolving()` harness (`src/resolver/mod.rs:920-1015`) passes the lockfile straight to `Resolver::new` — the right level, since partial upgrade is a Resolver behavior. Add sibling `resolving_partial_upgrade()`:
   - New fixture dir `src/tests/partial_upgrade/`, 4-section format: `config --- repos --- lockfile --- unlock-names` (one per line); extend `extract_test_elements` (`mod.rs:834-869`) or add a wrapper.
   - Call `resolver.unlock_packages(names)` before `resolve()`; snapshot the same debug output (`from_lockfile` visible in Debug).
   - Fixtures: (a) X upgradeable, unconstrained → moves, rest `from_lockfile: true`; (b) X dep of locked `my.pkg` whose req excludes newer X → X stays (key decision-6 scenario); (c) req satisfied by newer X → X moves, `my.pkg` stays locked; (d) newer X pulls brand-new transitive dep; (e) newer X drops a dep → GC; (f) multi-name unlock; (g) control: non-target with available bump stays locked; (h) builtin-sourced target.
2. **Unit tests `validate_upgrade_targets`**: no lockfile; single unknown; multiple unknowns together; dedupe; case-sensitivity; happy path.
3. **Compile-breadth**: dropping `Copy` is compiler-enforced across §1.2 sites; `cargo test` must pass with zero churn in existing resolver snapshots (empty skip-set ⇒ `Default`/`FullUpgrade` bit-identical).

## 7. Implementation sequence

1. `src/context.rs`: extend `ResolveMode` (drop `Copy`, add variant + `is_upgrade()`); switch to `&ResolveMode`; widen re-annotation.
2. `src/resolver/mod.rs`: `unlocked_packages` field + `unlock_packages()` setter + early-return in `lockfile_lookup`; wire from `Context::resolve`.
3. Mechanical `&ResolveMode` sweep: `src/cli/resolution.rs`, `src/cli/sync.rs`, all `src/main.rs` sites. `cargo check` clean; existing snapshots unchanged.
4. Resolver snapshot tests: fixture format extension + `resolving_partial_upgrade()` + fixtures (a)–(h).
5. New `src/cli/upgrade.rs`: `validate_upgrade_targets` + unit tests; export from `src/cli/mod.rs`.
6. `src/main.rs`: clap change + handler rewrite; validation before `load_for_resolve_mode`.
7. End-to-end manual verification: `rv upgrade <pkg>`, `--dry-run`, bare `rv upgrade`, unknown name, no-lockfile project; confirm lockfile diffs touch only the target subgraph.

### Critical files
- `src/context.rs`
- `src/resolver/mod.rs`
- `src/main.rs`
- `src/cli/sync.rs`
- `src/lockfile.rs`
