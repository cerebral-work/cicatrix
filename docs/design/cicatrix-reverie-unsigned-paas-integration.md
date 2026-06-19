# cicatrix ↔ reverie ↔ unsigned-paas — integration design & sequencing

> **Status:** design of record, drafted 2026-06-18.
> **Decisions captured via operator interview (2026-06-18):**
> **D-store** = *reverie is the single fact store* (drop the janus-datalog sidecar for v1).
> **D-target** = *the networked bridge waits for the cloud deploy path* (gated on CER-1362 → OPS-271).
> **D-scope** = this doc is the three-way design + sequencing; it does not itself ship code.
>
> Prior art (read, not duplicated): `unsigned-paas/docs/specs/reverie-cloud-roadmap.md` (R0–R5,
> decisions of record) and `unsigned-paas/docs/architecture/reverie-integration-questions.md`.
> North star: **CER-1369** (reverie as the single holistic memory surface).

---

## 1. What "the integration" actually is — two legs meeting at reverie

There is no single "reverie/cicatrix/unsigned-paas integration." There are **two legs that meet
at reverie**, plus a **consumer** that closes the loop:

```
  cicatrix ──(Leg A: bug-fact bridge)──▶  reverie  ◀──(Leg B: cloud deploy)── unsigned-paas
   (producer of regression-memory)      (the memory     (containerize + deploy reveried,
                                          surface)        authed + reachable)
                                            │
                                            ▼
                                         revenant  (consumer: recall at agent-spawn time, TOD-978)
```

- **Leg A — cicatrix → reverie.** Every fixed-bug fact becomes a reverie *observation*; "does this
  diff touch a known-bug surface?" becomes a reverie *search*. Small, self-contained. **Currently a
  stub** (`record`/`query` in `src/store.rs` are unimplemented behind the `BugStore` trait).
- **Leg B — reverie → unsigned-paas.** Get reveried containerized, deployed to the VKE cluster,
  reachable + authed. **Large, roadmapped, mostly unbuilt** — `reverie-cloud-roadmap.md` phases
  R0–R5; Linear project *"reverie-cloud platform prereqs"* (OPS-179–192). Live blocker:
  **CER-1362** (Urgent, In Progress) — *the reveried image is never built/pushed/signed*, so
  `helm install` would `ImagePullBackOff`. **OPS-271** (filed 2026-06-18) scopes the minimum
  deploy deps for the first ambient-recall/query path.
- **Consumer — revenant.** **TOD-978**: revenant's first orchestration path that consumes the
  surface for query + context injection. This is where cicatrix's value actually lands on agents.

The **end state** (CER-1369): cicatrix's regression-memory is *one content type* flowing into
reverie-the-surface, served from the unsigned-paas cluster, injected into agents by revenant.

---

## 2. Store model — reverie is the single store (D-store)

cicatrix keeps **no separate fact database**. Its durable artifacts are exactly two, in a strict
source→projection relationship (this is what keeps it from being cicatrix's own meta-pattern #2,
"two implementations of one fact drift"):

| Artifact | Role | Canonical? |
|---|---|---|
| `docs/bugs/resolved/BUG_*.md` | Human-authored fact, version-controlled in the repo | **Source of truth** |
| reverie observation (one per bug) | Queryable, embeddable projection of the fact | Derived, **regenerable** |

**The projection is one-way and idempotent:** markdown → reverie, deterministic, re-runnable at
will (same discipline as the roadmap's "JSONL importer re-run, no opaque blobs"). The **read path
(`query`) only ever reads reverie**; reverie is rebuildable from the markdown corpus at any time.
The markdown is canonical; reverie is a build artifact. No split-brain — there is one writer of the
truth (the markdown) and one read index (reverie), and the index is disposable.

**The janus-datalog sidecar is dropped from v1.** The `BugStore` trait in `src/store.rs` stays as
the seam, but its **only** implementation becomes `ReverieBridge` (an HTTP client to reveried).
`JanusStore` is removed from the substrate docs and the README's "decided" block is amended.

### 2.1 AsOf(commit) via git-ancestry — preserved in v1, without janus (decided: build now)

cicatrix's original pitch included janus-datalog's time-travel — "query the bug graph *as of any
past commit*." Reverie has no native `AsOf(commit)` evaluation, but **we keep the capability in v1
without a second store**, by layering the temporal filter in cicatrix over git:

- Every bug observation already carries its **`fix-commit`** as a tag (§3.1).
- `cicatrix query --as-of <commit>` post-filters reverie's results to facts whose `fix-commit` is an
  **ancestor** of `<commit>` — a local `git merge-base --is-ancestor <fix-commit> <commit>` check.
  "Was this bug known as of X?" = "was its fix already in X's history?"

The store stays reverie; the temporal logic is pure git in cicatrix (it owns the repo). This
dissolves the only real capability dropping janus would have cost — at the price of a few
`git merge-base` calls per query, not a Datalog engine. The default query (no `--as-of`) is
present-tense against the live set. **Edge cases to test** (cicatrix meta-pattern #3): a `fix-commit`
not reachable from `<commit>` (different branch), an unborn/empty repo, and a `fix-commit` that *is*
`<commit>` (inclusive boundary).

---

## 3. Leg A design — the cicatrix ↔ reverie bridge

Reverie already exposes the surface this needs (verified in `crates/reverie-store/src/http/`):
`POST /observations` (ingest), `GET /search` + `/search/v2` (recall), `/context/smart` (tiered
recall), bearer-scope auth. A reverie `Observation` carries `kind` + `title` + `content` +
`project` + `tags` (+ `topic_key`, `scope`, `valence`) — **no arbitrary structured-metadata
field.** So a bug-fact is encoded as embeddable text plus exact-match handles. **No reverie schema
change is required for v1.**

### 3.1 Projection schema (bug-fact → reverie observation)

| Observation field | Value from the bug-fact |
|---|---|
| `project` | `"cicatrix"` (the namespace; the query path filters on it) |
| `title` | the bug slug, e.g. `BUG_EMBED_EMPTY_INPUT_400` (stable handle for idempotent update) |
| `content` | the rendered fact: symptom · root-cause · fix-commit · regression-test · meta-pattern — embeddable, so semantic `/search` matches a diff's prose/symbols |
| `tags` | exact handles: each affected **file path**, the **meta-pattern** class, the **fix-commit** — the precise filter surface for `query` |
| `topic_key` | the meta-pattern class (groups a failure family for consolidation) |

**Idempotency:** the bug slug (`title`/`topic_key`) is the stable key; re-running `record` updates /
supersedes the existing observation (reverie has `revision_count` / `supersedes` machinery) rather
than duplicating.

### 3.2 The two verbs, made real

- **`cicatrix record`** (one mutating action): (1) append/update `docs/bugs/resolved/BUG_*.md`
  [canonical write]; (2) parse it to a `BugFact`; (3) `POST /observations` the projection
  [derived write]; (4) re-roll the CLAUDE.md meta-patterns section. Steps 1+4 are local and
  already partly exist; step 3 is the new `ReverieBridge` call.
- **`cicatrix query <changed-files>`**: `GET /search` (project=`cicatrix`) with the changed files
  and their symbols as the query; rank by tag-exact (file path hit) then semantic. Returns
  `Vec<BugFact>` → *"this diff touches known-bug surface X (meta-pattern Y); its regression test is
  Z — don't reintroduce it."* With `--as-of <commit>`, results are ancestry-filtered per §2.1
  (review an old branch as the bug-corpus looked then).

The **one capability to verify when wiring Phase 1**: that reverie `/search` can filter/boost by
`tags` precisely enough for the file-path exact-match (vs. relying on full-text over `content`). If
not, that is a small reverie-side ask, filed then — not a v1 blocker today.

### 3.3 What changes in the cicatrix crate

- `src/store.rs`: remove the `JanusStore` framing; implement `ReverieBridge: BugStore` (HTTP
  client, endpoint from `REVERIE_URL` / bearer from env). `meta_patterns()` stays.
- New `src/bug_md.rs`: parse `docs/bugs/resolved/*.md` per `_SCHEMA.md` → `BugFact`. **Pure,
  testable, no network** — this is the Phase-0 unblocked work.
- README + CLAUDE.md substrate blocks: amend "datalog store + reverie bridge" → "reverie is the
  store" with the §2 source/projection framing.

---

## 4. Sequencing — gated on the cloud path (D-target), but Phase 0 is unblocked now

"Wait for the cloud path" applies to the **networked** bridge. It must **not** mean cicatrix idles:
the pure, local prep can land now so Phase 1 is a thin wiring step the day reveried is reachable.

### Phase 0 — cicatrix prep (UNBLOCKED — do now, parallel to Leg B)
No reveried required.
- `src/bug_md.rs` markdown→`BugFact` parser + tests (locks the projection's input shape).
- Define the §3.1 projection schema as code (the payload builder), unit-tested against a fake.
- Drop janus-datalog from `store.rs`/README/CLAUDE.md; make `ReverieBridge` the sole planned impl.
- **Gate-honest:** `record` writes markdown + builds the fact + re-rolls meta-patterns *today*;
  it just doesn't POST yet (prints the would-be payload). No premature "wired" claim.
- **Exit:** `cargo test` green (the §`.cicatrix/establish-baseline.sh` gate); a recorded bug yields
  a correct observation payload, asserted in a test.

### Phase 1 — wire the bridge (GATED: reveried reachable — Leg B R0→R1, or local reveried up)
- Implement `ReverieBridge` HTTP `record`/`query` against `REVERIE_URL`.
- Verify §3.2's tag-filter assumption against the real `/search`; file a reverie ticket if short.
- **Exit:** record a seed bug → query a diff that touches its file → the bug surfaces. End-to-end,
  against whichever reveried endpoint exists.

### Phase 2 — cluster-served (GATED: Leg B R4 authed ingress + OPS-271 query path)
- Point cicatrix at the **cluster** reveried over authed ingress (`reveried.dev.unsigned.gg` or the
  tailnet hostname). Bug-memory is now part of the deployed ambient-recall surface.
- **Exit:** `query` works from a machine that is not the laptop, through auth.

### Phase 3 — revenant consumption (GATED: TOD-978)
- `cicatrix inject` graduates from a static string to a **live reverie recall**: at agent-spawn,
  revenant calls reverie (which now includes cicatrix bug-facts) to inject "known-bug surfaces near
  your task" into the agent's context. **This closes the CER-1369 loop** — regression-memory
  becomes ambient, queryable at authoring time, which was cicatrix's entire premise.
- **Exit:** an agent spawned by revenant on a task touching a known-bug file receives the warning
  inline, without anyone running `cicatrix query` by hand.

### The critical path (why the order is forced)
```
CER-1362 (image built/pushed/signed)
   └─▶ reveried deployable ─▶ OPS-271 (min deploy deps: reachable+queryable)
          └─▶ [Phase 1 bridge] ─▶ R4 authed ingress ─▶ [Phase 2] ─▶ TOD-978 ─▶ [Phase 3]

  [Phase 0 cicatrix prep]  ── not gated; runs in parallel with the whole top row ──┘
```

---

## 5. Cross-repo dependencies & tickets

| Need | Where | Ticket / artifact | Blocks |
|---|---|---|---|
| reveried image built/pushed/signed | reverie CI + unsigned-paas | **CER-1362** (Urgent, In Progress); OPS-179 (Harbor + kaniko) | all cloud phases |
| min deploy deps for ambient-recall/query path | unsigned-paas | **OPS-271** | Phase 1 (cloud), Phase 2 |
| reverie = single memory surface (north star) | reverie | **CER-1369** | the whole framing |
| revenant consumes the surface | revenant | **TOD-978** | Phase 3 |
| directive tier / static fallback | reverie | CER-1370 | adjacent (CLAUDE.md/rules path) |
| proactive notification policy | reverie | CER-1371 | adjacent (how Phase 3 surfaces) |
| `/search` tag-precise filtering | reverie | *file in Phase 1 if short* | Phase 1 exact-match quality |
| **cicatrix bridge itself** | cicatrix | *no ticket yet* — **file it** | Phases 0–3 of Leg A |

cicatrix is **not yet ticketed** anywhere. Leg A needs its own Linear issue(s); recommend a small
epic in the Reverie project (or Todie) with Phase 0 / Phase 1 as the first two.

---

## 6. Decisions (resolved — operator interview 2026-06-18)

1. **Reverie namespace:** **dedicated `project="cicatrix"`** — clean isolation, own recall scope,
   independently rebuildable / wipeable.
2. **AsOf(commit):** **build the git-ancestry filter now** (§2.1) — capability preserved in v1
   without janus, via the `fix-commit` tag + local `git merge-base --is-ancestor`. This adds a
   `--as-of` flag and the §2.1 edge-case tests to Phase 1 scope.
3. **Tracking:** **new small epic in the Reverie project, linked to CER-1369**, with Phase 0 /
   Phase 1 as the first two issues. (Spec drafted below / in chat; awaiting operator OK to file.)
```
