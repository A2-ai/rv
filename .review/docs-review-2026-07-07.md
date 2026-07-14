# Documentation-site review — `rv-docs` vs `rv` v0.22.1 (2026-07-07)

Reviewer scope: structure, missing content, incorrect content, and incorrect concepts in
`rv-docs` (Astro/Starlight site at `/Users/wescummings/projects/rv-docs`), verified against
the `rv` repo at v0.22.1 using `.review/survey-2026-07-07.md` as the anchor. Docs paths are
relative to `rv-docs/src/content/docs/`; source paths relative to the `rv` repo root.

---

## 1. Architecture summary

The site is a standard Starlight docs site: ~35 MDX pages under six sidebar sections
(Introduction, Project Configuration, Commands, Cookbook, Reference, Concepts), all
navigation hand-maintained in `astro.config.mjs` (sidebar defined at lines 18–150). A few
shared assets (`src/configs/*.toml`, `src/assets/other/*`) are imported raw into pages —
a good single-sourcing pattern used in `intro/getting-started.mdx` and
`concepts/resolution.mdx`. There is **no generated content**: every command page, option
list, and config-field list is hand-written prose, even though the binary ships a
machine-readable CLI model precisely for this purpose (`rv docs cli`, `src/main.rs:229`,
`src/cli_docs.rs`).

Flows traced: (1) new-user path `index → intro/getting-started → config/intro →
commands/sync` — coherent, accurate, well cross-linked; (2) command pages vs the clap
definitions in `src/main.rs:51–246` — this is where nearly all drift lives; (3) reference
pages (`reference/env_vars.mdx`) vs `src/consts.rs` — one missing variable, otherwise
consistent.

Overall read: the conceptual content (resolution semantics, lockfile precedence, caching
model, renv comparison) is accurate and unusually good — I spot-checked the resolution
examples against resolver behavior and the `prefer_repositories_for` conditions match the
source doc-comment (`src/config.rs:336–343`) verbatim. The structural weakness is
**release lag with no drift defense**: the CLI/config surface documented is roughly
v0.18-era, while v0.19–v0.22 added three commands, four config fields, several flags, and
one env var that the site doesn't mention at all. Since `README.md` defers all usage
documentation to this site (survey §1.8), undocumented ≈ invisible.

---

## 2. Primary findings (ranked)

### F1. Three shipped commands have no page; `rv remove` is linked and 404s
**Severity: High · Category: missing content / broken structure · Confidence: high**

- Evidence: `commands/intro.mdx:39` links `[rv remove](../remove)`; no
  `commands/remove.mdx` exists (file inventory + sidebar in `astro.config.mjs`).
  `Command::Remove` (`src/main.rs:106`), `Command::Run` (`src/main.rs:215`),
  `Command::Export`/`ExportSubcommand::Renv` (`src/main.rs:78`, `:328–336`) are all real,
  released commands (CHANGELOG v0.21.0, v0.22.0).
- Issue: `rv remove` is half-referenced (intro lists it, sidebar and page don't), so the
  link 404s. `rv run` and `rv export renv` appear nowhere on the site — not in the
  commands intro, sidebar, or any cross-reference. `rv export renv` is the designated
  escape hatch back to renv, a significant adoption-risk reducer that the
  renv-difference page should be selling.
- Blast radius: users can't discover 3 of ~19 commands from the only user-facing docs;
  the broken link is on the second-most-trafficked commands page.
- Remediation: add `commands/remove.mdx` (mirror `add.mdx`: `--dry-run`, `--no-sync`,
  `src/main.rs:106–117`), `commands/run.mdx` (syncs by default since v0.22, `--no-sync`
  must be first flag), and `commands/export_renv.mdx` (note R-universe → git conversion
  on export); wire all three into the sidebar and intro. ~half a day.

### F2. Config schema drift: four `[project]` fields exist that the docs implicitly deny
**Severity: High · Category: missing/incorrect content · Confidence: high**

- Evidence: `src/config.rs:310–364` (`Project` struct) has `use_devel`
  (`src/config.rs:319`, consumed by `src/r_finder.rs:245`), `no_strip`
  (`src/config.rs:355–359`, v0.19), `git_shorthand_base_url` (`src/config.rs:360–363`,
  v0.22), and templated `library` paths — `{r_version}`/`{name}` placeholder expansion at
  `src/config.rs:530–531` (v0.22). None appear anywhere in the site (grep across all MDX:
  zero hits). Meanwhile `config/intro.mdx:18–95` presents itself as the exhaustive field
  enumeration, and `config/library.mdx` documents the `library` field with no mention of
  templating (or of the `RV_LIBRARY_DIR` override that beats it).
- Issue: an enumerating intro page turns omissions into implicit "this doesn't exist."
  `no_strip` also implies undocumented default behavior (rv passes `--strip`/
  `--strip-lib` to `R CMD INSTALL`) that users hitting stripped-binary bugs need to know.
- Blast radius: anyone needing per-R-version shared libraries (templating is *the*
  answer to the caveat `config/library.mdx:9–10` itself raises about name-spacing),
  devel-R users, and GitHub-Enterprise orgs (shorthand base URL) can't find the feature.
- Remediation: add the four fields — templating and `RV_LIBRARY_DIR` cross-link into
  `config/library.mdx`; `no_strip` as a new Package Compilation entry (documenting the
  strip default); `use_devel` into `config/r_version.mdx`; `git_shorthand_base_url` next
  to the `rv add` shorthand (F3). ~1 day.

### F3. CLI option drift: `--locked`, `rv add owner/repo` shorthand, `--commit`, `--emit-events`
**Severity: High · Category: missing content · Confidence: high**

- Evidence: `--locked` on `Sync` (`src/main.rs:89`) and `Plan` (`src/main.rs:133`,
  mutually exclusive with `--upgrade`, `src/main.rs:837`) — absent from
  `commands/sync.mdx:15–16` (which documents only `--save-install-logs-in`) and
  `commands/plan.mdx:14–17`. `rv add` accepts `owner/repo[@ref][:subdir]` shorthands
  (`src/main.rs:103` doc-comment; CHANGELOG v0.22.0) and a `--commit <SHA>` option
  (`src/dependency_edit.rs:34–36`) — `commands/add.mdx:20–36` documents neither, and
  `cookbook/rv_add_examples.mdx` has no shorthand example. Global `--emit-events`
  (`src/main.rs:37–40`, NDJSON progress stream) is absent from the global-options list
  in `commands/intro.mdx:72–79`, which does document its sibling `--json`.
- Issue: `--locked` is the CI/reproducibility flag — exactly the audience that reads
  docs rather than `--help`. The `--commit` omission is worse than neutral:
  `config/dependencies.mdx:68` teaches "tag or branch or commit — must specify one,"
  so readers will reasonably try `rv add --git … --commit …` and find it undocumented.
- Blast radius: CI adopters, anyone pinning git SHAs via `rv add`, IDE integrators.
- Remediation: add `--locked` to sync/plan pages (one shared aside on CI usage), the
  shorthand + `--commit` to `add.mdx` and one cookbook example, `--emit-events` to the
  global options list (or explicitly mark it internal — see Q2). ~half a day.

### F4. Copy-pasteable commands that fail: wrong flag spellings and malformed usage lines
**Severity: Medium · Category: incorrect content · Confidence: high**

- Evidence (three independent locations, same defect class):
  - `commands/init.mdx:36` and `commands/migrate_renv.mdx:50` document
    `--no_r_environment`; clap derives kebab-case, so the real flag is
    `--no-r-environment` (`src/main.rs:65–67`, `:333`; `commands/activation.mdx:19`
    spells it correctly, proving intra-site inconsistency).
  - `commands/configure_repos.mdx:222–224`: the `remove` usage line reads
    `rv configure repository <ALIAS> [OPTIONS]` — missing the `remove` subcommand.
  - `commands/configure_repos.mdx:195`:
    `rv configure repository update --match-url … -- url https://cran.rstudio.com` —
    `-- url` (space) instead of `--url`; as written, clap treats `url` and the URL as
    positional args and the command errors.
- Blast radius: each is a command a user copies verbatim and gets a clap error from;
  the `--no_r_environment` one appears in two onboarding-critical pages.
- Remediation: fix the three strings; then add a CI check that extracts fenced `shell`
  blocks starting with `rv ` and validates flags against `rv docs cli --format json`
  (the machinery already exists, `src/cli_docs.rs`). Fixes: minutes; the guard: ~a day.

### F5. sysdeps platform support is misstated and inconsistent across three pages
**Severity: Medium · Category: incorrect content · Confidence: high**

- Evidence: `commands/sysdeps.mdx:8` — "**Only available for Ubuntu and Debian.**";
  `commands/intro.mdx:62` — "(only for Ubuntu/Debian)". But
  `src/system_req.rs:79–93` (`is_supported`) covers ubuntu, debian, centos 7/8,
  redhat 7/8/9, rockylinux 8/9, opensuse/SLE 15, with rpm-based presence checks at
  `src/system_req.rs:238–249` and RHEL-family alias mapping in
  `src/system_info.rs:114–166`. `reference/env_vars.mdx:28,34` and
  `commands/summary.mdx:65` say "Ubuntu, Debian, and RHEL-like" — closer, but all three
  formulations disagree and none mentions SUSE.
- Note for maintainers: the clap doc-comment on `Sysdeps` (`src/main.rs:190–192`) is
  itself stale ("only supported on Ubuntu/Debian") — worth fixing at the source too,
  since `--help` and any generated docs inherit it.
- Blast radius: RHEL/Rocky users — a key pharma deployment target given
  `ai-docs/2025-10-18-rhel-research.md` — are told the feature won't work for them.
- Remediation: define the supported matrix once (a small table on the sysdeps page,
  sourced from `is_supported`), link the other three pages to it. ~1–2 hours.

### F6. `config/prefer_repositories_for.mdx` example config cannot parse
**Severity: Medium · Category: incorrect content · Confidence: high**

- Evidence: `config/prefer_repositories_for.mdx:24–38` — the example puts `name`,
  `r_version`, `repositories`, `dependencies`, and `prefer_repositories_for` all at the
  TOML top level with no `[project]` table. The top-level `Config` struct only accepts
  `library`, `use_lockfile`, `lockfile_name`, `project` and is
  `#[serde(deny_unknown_fields)]` (`src/config.rs:373–380`), so this file is rejected
  with an unknown-field error on `name`. Every other config page on the site correctly
  shows `[project]` (e.g. `config/packages_env_vars.mdx:21–33`).
- Blast radius: this is the reference page for one of rv's flagship differentiators;
  the cookbook (`cookbook/remotes.mdx`) gets it right, but a user starting from the
  config page copies a file that hard-fails.
- Remediation: add the `[project]` header (and, ideally, run all doc TOML snippets
  through `Config::from_file` in a docs CI job — the fixtures pattern in
  `example_projects/` shows how). Fix: minutes.

### F7. `reference/env_vars.mdx` missing `RV_INSECURE`; `RV_LINK_MODE` entry under-specified
**Severity: Medium · Category: missing content · Confidence: high**

- Evidence: `RV_INSECURE` exists (`src/consts.rs:29`; disables TLS verification with a
  one-time warning, `src/http.rs:45–52`; CHANGELOG v0.19.0) — zero hits in the docs
  site. It is the answer to the very common corporate-proxy/custom-CA failure mode.
  The `RV_LINK_MODE` section (`reference/env_vars.mdx:42–45`) states only the OS
  defaults; it omits the accepted values (`copy`/`clone`/`hardlink`/`symlink`), the
  network-FS auto-selection of symlink, and the fall-back-to-copy behavior — all of
  which `rv/CLAUDE.md` ("RV_LINK_MODE values" table) and `src/sync/link.rs:44–56,65+`
  specify. The defaults stated (hardlink Windows/Linux, CoW macOS) are correct.
- Blast radius: proxy-blocked users file bugs instead of finding the escape hatch;
  NFS users can't reason about why their installs behave differently.
- Remediation: add an `RV_INSECURE` section (with the appropriate security warning) and
  port CLAUDE.md's link-mode value table + priority order (env var → network detection →
  OS default → copy fallback). ~1–2 hours.

### F8. `concepts/cache.mdx` factual errors: wrong env var name, wrong DB filename, inconsistent example
**Severity: Medium · Category: incorrect content · Confidence: high**

- Evidence:
  - `concepts/cache.mdx:92` — "use the [`GHQC_CACHE_DIR` env var]" — wrong project's
    variable (copy-paste from the ghqc codebase); the link target and `src/consts.rs:26`
    say `RV_CACHE_DIR`.
  - `concepts/cache.mdx:56,65,70` — file tree shows `packages.bin`; the actual package
    database file is `packages.mp` (`src/consts.rs:19`, MessagePack).
  - `concepts/cache.mdx:51` vs `:62–77` — the `rv cache` output line shows the git
    binary under `4.4/arm64` while the config and the file tree use R 4.5.
  - `concepts/cache.mdx:123–126` — the four-step fetch order omits the v0.19 binary
    archive fallback (check repo binary archive before source compilation, CHANGELOG
    v0.19.0), so the described behavior under-sells and mis-predicts what users see.
- Blast radius: this page is the reference admins use when debugging shared caches;
  a wrong env var name and wrong filename cost real diagnosis time.
- Remediation: fix the name/filename/version, insert the archive-fallback step. ~1 hour.

### F9. `commands/init.mdx` shows a stale generated config; the shipped template has changed
**Severity: Low · Category: incorrect content · Confidence: high**

- Evidence: the "Config File" tab (`commands/init.mdx:80–101`) shows old template
  comments ("any CRAN-type repository…", dependency examples including a `path` line).
  The actual template (`src/cli/commands/init.rs:16–37`) has different comment text and
  now includes the commented `git_shorthand_base_url` line. The imported asset used by
  `intro/getting-started.mdx` (`src/configs/init_config.toml`) is also one revision
  behind (no shorthand comment). Additionally `commands/init.mdx:33–34` describes
  `--add` without noting it is repeatable (`--add pkg1 --add pkg2`); v0.21 explicitly
  changed the semantics away from space-separated lists (CHANGELOG v0.21.0 bug-fix +
  migration note), so the old mental model actively misleads.
- Remediation: regenerate the two snippets from a real `rv init` run and state the
  repeatable `--add` form. ~1 hour. (The durable fix is the snippet-refresh job in F4.)

---

Findings I trimmed to stay within ten, summarized: `commands/tree.mdx:16` describes
`--hide-system-deps` as "whether to display" (inverted — it hides,
`src/main.rs:169–172`); `commands/activation.mdx:27` shows `rv deactivate [OPTIONS]`
though `Deactivate` takes none (`src/main.rs:211`); `reference/env_vars.mdx` never
mentions that `RV_GLOBAL_CACHE_DIR`'s directory must pre-exist (CLAUDE.md documents
this); and `config/use_lockfile.mdx:10–13` is internally confusing ("Without this
configuration option… invalidate" vs "…does not invalidate") — the behavior claim is
right (custom-library content is ignored, `src/library.rs:142–144`, and lockfile-less
projects re-sync) but the paragraph needs a rewrite.

---

## 3. Low-hanging fruit

- `config/repositories.mdx:8` — "CRAN=like" → "CRAN-like".
- `intro/getting-started.mdx:15` — "will create generate the following" → pick one verb.
- `concepts/resolution.mdx:203` — `` `[use_lockfile]`(../../config/use_lockfile) `` —
  backticked link text breaks the markdown link; renders as literal text.
- `commands/configure_repos.mdx:122,230,265` — "[the first example](#example)" anchors
  point at `#example`, but the add-section heading is `### Examples` (`#examples`);
  the link resolves to the wrong heading or nothing.
- `reference/env_vars.mdx:44` — "re-downloading pages" → "packages"; `:50` — "ca be" →
  "can be".
- `config/packages_env_vars.mdx:14`, `config/configure_args.mdx:13`,
  `concepts/cache.mdx:133` — "effect other projects" → "affect".
- `commands/summary.mdx:7` — "how many packages need to be installed" duplicated intent
  with "how packages need to be installed"; rephrase.
- `commands/tree.mdx:16` — invert the `--hide-system-deps` description (see above).
- `commands/activation.mdx:27` — drop `[OPTIONS]` from `rv deactivate` usage.

## 4. Questions for maintainers

1. **Should command pages be generated?** `rv docs cli --format json/markdown`
   (`src/main.rs:224–246`, `src/cli_docs.rs`) looks purpose-built to feed this site,
   yet every command page is hand-written and the drift in F1/F3/F4 is exactly what
   generation prevents. Is adoption planned, or is hand-curation deliberate (for the
   narrative style)? A hybrid — hand-written prose + generated options tables — would
   keep both.
2. **Is `--emit-events` intentionally undocumented?** It's marked "Intended for IDE/GUI
   integration" (`src/main.rs:38`); if the event schema isn't stable, a one-line
   "internal, subject to change" mention still beats silence.
3. **`rv docs` itself** is visible in `--help` but experimental — document with a
   stability caveat, or hide it?
4. **R-Universe page** (`config/repositories.mdx:53–57`): implementation now fetches the
   R-Universe `api/packages` endpoint and models it as `Source::RUniverse`
   (`src/context.rs:318–335`, `src/resolver/dependency.rs:105`) rather than literally
   rewriting to git dependencies. The user-visible claim ("treated as git dependencies,
   compiled from source") still matches behavior — but is the "we may consider binaries
   in the future" note still the roadmap position?
5. **No lockfile reference page.** `rv.lock` appears in examples (`commands/upgrade.mdx`
   shows `version = 2`, which matches `CURRENT_LOCKFILE_VERSION`,
   `src/lockfile.rs:17`) but no page documents its format or the
   `Source` variants. Intentional (it's "not for manual editing"), or a gap?

## 5. What's good (preserve these)

1. **The Concepts/Resolution page is excellent and accurate.** The worked examples
   (repository ordering, lockfile precedence, `prefer_repositories_for` conditions)
   match resolver behavior and the source doc-comments exactly (`src/config.rs:336–343`).
   This narrative depth is something generated docs can't replace — keep it hand-written.
2. **Raw-imported shared assets** (`src/configs/*.toml`, `assets/other/*` imported with
   `?raw` into `getting-started` and `resolution`) are the right single-sourcing
   mechanism — extend that pattern (F9) rather than inlining more snippets.
3. **Task-oriented information architecture** — required fields vs compilation vs
   project options; commands grouped by workflow; cookbook separated from reference —
   maps well onto how users actually adopt rv. The gaps are freshness, not structure.
