# cicatrix as static-analysis on reverie's codebase — capability gap & integration design

> **Status:** design, drafted 2026-06-23. Companion to
> `cicatrix-reverie-unsigned-paas-integration.md` (which covers **Leg A**, the bug-fact
> bridge). This doc covers the **second arm** — cicatrix's *convention-drift / static-analysis*
> capability applied **to reverie**.
> **Premise correction up front:** "use cicatrix for static analysis on reverie" today means
> *build the scanner*, not *wire an existing one*. The drift arm is a hand-curated table plus a
> path-printer; no analysis code exists yet. This doc designs the thing that has to be built and,
> more importantly, decides **whether cicatrix should own it at all** given what reverie already
> ships.

---

## 0. TL;DR

1. **There is no scanner.** `cicatrix drift` prints the path of a static markdown table
   (`drift/convention-drift-2026-06-16.md`) hand-authored from a one-time topology survey. Its
   only test asserts the path exists. `Cargo.toml` has **no** AST/parsing deps (only `ureq` +
   `serde`). So "static analysis" is, today, a spreadsheet.
2. **"Static analysis" splits two ways and cicatrix does neither yet:**
   - **(a) Convention-marker / topology drift** — *does the repo have CLAUDE.md, a toolchain pin,
     a release config?* This is cicatrix's current data model — but as data, not as a scan.
   - **(b) Code-level convention enforcement** — *anyhow-in-bins/thiserror-in-libs,
     `reverie-domain` serde-free, error-type-by-layer.* Needs an AST or at least structured regex.
     cicatrix has none of that machinery.
3. **Reverie already has a dense static-analysis surface** (clippy `-D warnings`, cargo-deny,
   typos, gitleaks, a `domain-invariant-check.sh` regex gate, the `reverie-audit` crate, and a
   subcommand **literally named `cortex drift`**). Any cicatrix arm here must **complement, not
   duplicate** — and must resolve the **`drift` naming collision** before it confuses everyone.
4. **The real decision** (§5): does cicatrix *own* a convention scanner, or does reverie absorb the
   idea into `cortex` / `reverie-audit`? **Recommendation: cicatrix owns marker/topology drift
   (cross-repo, where reverie has nothing); reverie owns code-level convention checks (in-repo,
   where it already has the AST context and the CI gate).** They meet — like Leg A — at reverie the
   memory surface.

---

## 1. What exists today (ground truth, both repos)

### 1.1 cicatrix's "static-analysis" arm — current maturity

| Piece | File | Reality |
|---|---|---|
| `drift` command | `src/main.rs` | `"drift" => println!("drift/convention-drift-2026-06-16.md")` — **prints a path, runs no analysis** |
| Drift data | `drift/convention-drift-2026-06-16.md` | Hand-curated repo×marker table from a 2026-06-16 6-agent topology survey across ~11 repos |
| Drift test | `tests/cli.rs::drift_advertises_a_path_that_exists` | Asserts the advertised file exists on disk. The "drift-path invariant" = *don't point at a nonexistent artifact*. Nothing about scanning. |
| Analysis deps | `Cargo.toml` | `ureq`, `serde`, `serde_json` — **no tree-sitter / syn / regex / AST** |

**The convention-marker model** (the genuinely reusable part). Nine markers tracked per repo against
four canonical templates (`template-{rust,node,python,terraform}`):

```
CLAUDE  CLAUDE.md present                MK    Makefile/justfile w/ ci target
PC      .pre-commit-config.yaml          CI    GH Actions workflow count
LIC     LICENSE present                  TOOL  toolchain pin (rust-toolchain.toml/.nvmrc)
SR      signed-release config            CHG   CHANGELOG.md
```

Reverie's row in that table today: `rust ✓CLAUDE ✓MK ✓PC 6×CI ✓LIC ✓TOOL ✗SR ✓CHG` — i.e. the
survey's one flagged gap for reverie is **no signed-release config** (`release-please-config.json`).
That single cell is the entire current "static analysis of reverie." It is a finding, but it was
produced by humans/agents reading the repo once, not by a tool that can be re-run on a diff.

### 1.2 reverie's existing static-analysis surface (what cicatrix must not re-do)

Machine-enforced, gated in `make ci-check` / `.github/workflows/ci.yml` / `.claude/hooks/`:

| Surface | Tool | Where |
|---|---|---|
| Formatting | `cargo fmt --all -- --check` | pre-commit + ci-check |
| Linting | `cargo clippy --workspace --all-targets [--all-features] -- -D warnings` | pre-commit + ci-check |
| Advisories/licenses/sources | `cargo deny check` (`deny.toml`) | preflight job + ci-check |
| Typos | `typos` (`_typos.toml`) | preflight job + ci-check |
| Secrets | `gitleaks` (`.gitleaks.toml`) | pre-commit + CI job |
| **Domain invariant** | `scripts/ci/domain-invariant-check.sh` | CI job `domain_invariant` |
| **Runtime/doc drift** | `cortex drift docs` | CI job `docs_drift` |
| Corpus health | `reverie-audit` crate (7 checks) | post-analysis, not gated |

Two of these are the ones cicatrix collides with:

- **`scripts/ci/domain-invariant-check.sh`** — a regex gate forbidding `\bStoredObservation\b` in
  non-storage crates (keeps `reverie-domain` serde-free / wire-types at the edge). **This is already
  a code-level convention scanner.** It is exactly the category (b) work, done in bash, expandable
  ("StoredObservation is the first forbidden name").
- **`cortex drift`** — a subcommand **named `drift`** that detects *runtime* drift: binary vs
  installed vs running version, daemon `/health`, `PRAGMA user_version`, worktree staleness, coord
  orphans, plus `cortex drift docs` for code↔doc path validity. **Different semantics** from
  cicatrix's *convention* drift, but the shared word is a landmine.

---

## 2. The capability-reality gap (read this before scoping)

The request "use cicatrix for static analysis on reverie's codebase" assumes a capability cicatrix
does not have. Precisely:

- It **cannot** scan reverie for missing convention markers — the table was filled in by hand.
- It **cannot** check code-level conventions (no AST, no regex rules engine).
- What it **can** do today against reverie is exactly what Leg A already designs: record reverie's
  *resolved bugs* as facts and answer "does this diff touch a known-bug surface?" That is
  regression-memory, not static analysis — valuable, but a different thing, and already specced.

So this design is a **build** plan with a **make-or-absorb** decision at its center, not a wiring
task. Treating it as "turn it on" would misrepresent the state (cicatrix's own meta-pattern: *no
premature victory*).

---

## 3. Two arms of "static analysis", and who should own each

### Arm (a) — convention-marker / topology drift  → **cicatrix owns**

*"Across cerebral-work's repos, which are missing the conventions a normalized repo of their
language should have?"* Reverie is **one row** in this cross-repo table.

- **Why cicatrix, not reverie:** this is inherently **cross-repo**; reverie has *no* surface for it
  (reverie's checks are all in-repo). It matches cicatrix's existing data model and its
  janus-derived "convention-drift detection" charter. Reverie shouldn't grow a cross-repo scanner.
- **What to build (turns the table into a tool):**
  1. A `markers.toml` declaring each marker as a **presence/shape predicate** (file exists, file
     matches a pattern, GH workflow count ≥ N) per canonical template.
  2. A repo list with language labels (config, not hard-coded — reverie's CLAUDE.md warns against
     hard-coding the device/repo set).
  3. `cicatrix drift scan [--repo <path>]` — traverse, evaluate predicates, **regenerate**
     `drift/convention-drift-<date>.md` instead of printing a stale path. Keep the existing
     drift-path invariant test; add a "scan reproduces the committed table on an unchanged repo"
     test (cicatrix meta-pattern: *assert the invariant, not the happy path*).
  4. Pure traversal + predicate eval — still **no AST needed**; `std::fs` + `serde` + a little
     `regex` at most. Keeps the crate's minimal, safe dependency profile.
- **Reverie-specific output:** the scan confirms/refreshes reverie's `✗SR` cell and emits it as a
  finding — *"reverie is missing release-please-config.json vs template-rust."* That finding is the
  natural bridge payload (§4).

### Arm (b) — code-level convention enforcement  → **reverie owns; cicatrix defers**

*"anyhow-in-bins/thiserror-in-libs; `reverie-domain` serde-free; error-type-by-layer; no
`Co-Authored-By: Claude` trailer."* These are real gaps (prose-only in CLAUDE.md, mostly
unenforced), **but:**

- They need reverie's own context (workspace layout, which crate is a lib vs bin, `#[cfg(test)]`
  stripping) — reverie already does this in `domain-invariant-check.sh`.
- The right home is **extending `domain-invariant-check.sh`** (add a `#[serde` probe in
  `reverie-domain`; add a commit-msg hook rejecting the AI-attribution trailer) or a small
  `cortex lint-conventions` subcommand — **not** a second tool in a sibling repo reaching across
  the boundary. A cross-repo tool enforcing reverie's *internal* code conventions would itself be
  cicatrix meta-pattern #2 ("two implementations of one fact drift").
- **cicatrix's contribution here is the corpus, not the checker:** when one of these conventions is
  *violated and fixed*, that fix becomes a `BUG_*.md` regression-memory fact (Leg A), so the next
  agent is warned at authoring time. cicatrix supplies memory; reverie supplies the gate.

This split is the load-bearing recommendation. It keeps each check where its context lives.

---

## 4. How the arm (a) findings reach reverie — same shape as Leg A

Convention-drift findings ride the **exact bridge Leg A already builds** (`src/reverie.rs`,
`ReverieBridge: BugStore`, `POST /observations` / `GET /search`, `project="cicatrix"`). No new
transport, no reverie schema change. A drift finding projects to an observation:

| Observation field | Drift finding value |
|---|---|
| `project` | `"cicatrix"` (or a `cicatrix-drift` topic split if recall noise warrants) |
| `type` | `"convention-drift"` (parallel to `"bug-fact"`) |
| `title` | stable handle, e.g. `DRIFT_REVERIE_NO_RELEASE_CONFIG` |
| `content` | rendered finding: repo · language · marker · expected-from-template · observed | embeddable |
| `tags` | `repo=reverie`, `marker=SR`, `template=template-rust` (exact-match filter surface) |
| `topic_key` | the marker class (groups the failure family for consolidation) |

This means "static analysis of reverie" lands in the **same memory surface** (CER-1369) as the bug
facts — queryable, consolidatable, eventually injected by revenant at agent-spawn (Leg A Phase 3).
The `--as-of <commit>` git-ancestry filter (`src/gitf.rs`) applies unchanged: *"was this convention
gap already true as of commit X?"*

---

## 5. The decision (make explicit, don't bury)

**Q: Does cicatrix own a convention scanner, or does reverie absorb it?**

**A (recommended): split by locus, not by tool affinity.**

- **cicatrix owns cross-repo marker/topology drift** — the thing only it is positioned to do, where
  reverie has nothing, where its data model already fits. Reverie is one scanned row.
- **reverie owns in-repo code-level convention checks** — extend `domain-invariant-check.sh` /
  `cortex`, where the AST/workspace context and the CI gate already live.
- **cicatrix's bug-memory (Leg A) is the connective tissue:** fixed convention violations become
  regression-memory facts; both arms project into reverie the surface.

Rejected alternatives:
- *cicatrix builds a full code-level linter for reverie* — duplicates clippy/domain-invariant,
  reaches across a repo boundary for internal conventions, no AST machinery, high cost.
- *reverie builds the cross-repo scanner in cortex* — wrong locus; cortex is reverie-runtime-shaped;
  cross-repo topology is cicatrix's charter.

### 5.1 Resolve the `drift` naming collision (do this regardless)

`cortex drift` (reverie, runtime/doc drift) and `cicatrix drift` (convention drift) will collide in
conversation and docs. Options, in order of preference:
1. **cicatrix `drift` → `drift scan` / `conventions`**, and always qualify as
   *"convention drift"* in prose; reserve bare "drift" in reverie contexts for `cortex drift`.
2. Cross-link both docs with a one-line "not to be confused with" note.
Pick (1); cheap, prevents a year of confusion.

---

## 6. Sequencing

| Phase | Work | Gated on |
|---|---|---|
| **D0 (now, unblocked)** | `markers.toml` + repo-list config; `cicatrix drift scan` regenerates the table from real traversal; rename `drift`→`drift scan`; tests (reproduce-on-unchanged + drift-path invariant). **No reveried needed.** | nothing |
| **D1** | Project a drift finding → reverie observation (`type=convention-drift`) over the existing `ReverieBridge`. Reuse Leg A Phase 1's wiring. | reveried reachable (Leg A Phase 1) |
| **D2** | reverie side: extend `domain-invariant-check.sh` (serde-free probe; AI-attribution-trailer commit-msg hook) — **a reverie PR, not cicatrix.** | independent |
| **D3** | Convention findings consolidated + injected with bug-facts via revenant. | Leg A Phase 3 (TOD-978) |

D0 + D2 are parallel and unblocked. D1/D3 ride Leg A's gates — no new external dependency.

---

## 7. Cross-references

- **Leg A (the bridge), store model, `--as-of`, cloud sequencing:**
  `cicatrix-reverie-unsigned-paas-integration.md` (this doc does **not** re-derive them).
- **Epic skeleton:** `cicatrix-bridge-epic.spec.md`.
- **North star:** CER-1369 (reverie = single holistic memory surface).
- **Reverie surfaces referenced:** `Makefile` (`ci-check`), `scripts/ci/domain-invariant-check.sh`,
  `crates/cortex/src/drift.rs`, `crates/reverie-audit/`, `.claude/hooks/`, `deny.toml`,
  `_typos.toml`, `.gitleaks.toml`.
- **Lineage:** the convention-drift charter derives from `wbrown/janus-datalog`'s agent-discipline
  layer; cicatrix keeps the *idea* (queryable discipline) while dropping the datalog store
  (decided 2026-06-18: reverie is the store).

---

## 8. Open questions

1. ~~Where do the **canonical templates** (`template-rust` …) physically live?~~ **Resolved
   (2026-06-23): they live in the `cerebral-work` repo.** D0 sources its marker predicates from
   there. (Note the survey flagged `cerebral-work` itself as a "complete void" — confirm the
   templates are actually populated before authoring predicates against them.)
2. Separate reverie `topic_key`/project for convention-drift vs bug-fact, or one `cicatrix`
   namespace? Decide by measuring recall noise once D1 lands (defer; not a D0 blocker).
3. Should the cross-repo scan run in CI anywhere, or stay an operator-invoked sweep? (cicatrix has
   no remote-CI story for scanning *other* repos; likely a scheduled local/cluster job post-cloud.)
