# BUG_YAML_ANCHOR_REPARENTS_SIBLINGS

- **id:** bug:yaml-anchor-reparents-siblings
- **files:** charts/revenant/values.yaml, charts/revenant/templates/journal-pvc.yaml
- **fix-commit:** revenant 4d0f2a8 (chart 0.2.6); introduced by revenant#162 (chart 0.2.5)
- **regression-test:** `charts/revenant/tests/values-shape.sh` — asserts `godseat.journal.size` and `.storageClassName` are present and that no template reads a value the file does not define
- **meta-pattern:** A text-anchored edit is not a structural edit
- **status:** resolved
- **scope:** charts/

## Symptom

An ArgoCD sync of `revenant-coordinator` failed on one object while the rest of the
release applied cleanly:

```
PersistentVolumeClaim "revenant-godseat-journal" is invalid:
  spec.resources[storage]: Required value
```

The app went `OutOfSync` and every subsequent sync would have failed on the same
object. Nothing was damaged — a bound PVC's spec is immutable, so the bad render
could not corrupt the live 1Gi volume — but the release was no longer applicable.

Critically, the chart had already passed `helm lint`, code review, merge, and
publication to Harbor as an immutable version (0.2.5) before anyone saw this.

## Root cause

A scripted edit inserted a new `llmKeySecret:` block into `values.yaml` using the
line `dir: /var/lib/godseat/journal` as its anchor. The block it was inserting
into looked like this:

```yaml
  journal:
    dir: /var/lib/godseat/journal
    size: 1Gi
    storageClassName: longhorn
```

The anchor matched exactly once, the replacement "succeeded", and the new block
landed **between `dir:` and its two siblings**. Because YAML nesting is decided
by indentation relative to the nearest preceding key, `size` and
`storageClassName` were silently re-parented from `godseat.journal` into
`godseat.llmKeySecret`.

`journal-pvc.yaml` reads `.Values.godseat.journal.size` and
`.Values.godseat.journal.storageClassName`. Both were now missing, so Helm
rendered them as empty strings — which is a *valid document* containing an
*invalid PVC*.

**The mental-model error:** treating a YAML file as text with a unique anchor,
when the thing being edited is a tree. A single-line anchor carries no
information about what follows it, so an insert after that line is an insert
into an unknown structural position. The edit was verified by "did the anchor
match?" — a question that cannot detect re-parenting.

**Why every gate missed it.** `helm lint` and `helm template` both check that the
chart *renders*, not that the values templates reference actually exist. Helm
resolves a missing value to the empty string rather than erroring, so an
orphaned key is indistinguishable from a deliberately blank one at render time.
The failure is only observable where the rendered object meets a schema — the
API server — which is to say, at deploy.

## Reproduction

```bash
# In a chart whose values contain a mapping with >1 child:
#   parent:
#     first: a
#     second: b
# insert a new sibling block anchored on `first:` only.
python3 - <<'PY'
s = open('values.yaml').read()
s = s.replace("  parent:\n    first: a", "  parent:\n    first: a\n  newblock:\n    x: 1")
open('values.yaml','w').write(s)
PY
helm lint .        # PASSES
helm template .    # PASSES — `second` now lives under `newblock`
# The break appears only when the API server validates the rendered object.
```

## Resolution

`size` and `storageClassName` restored under `journal:` where the template reads
them, and the orphaned copies removed from `llmKeySecret`. Chart republished as
0.2.6 — 0.2.5 was already published and versions are immutable, so the broken
render is permanently in the registry and must be superseded rather than fixed
in place.

The guard is a values-shape assertion that parses the YAML and checks the keys
templates actually consume, rather than checking that the chart renders.

## Lesson

**Validate the parsed shape, not the render.** For any config edit:

1. Prefer a structure-aware edit (parse → mutate → serialise) over text
   replacement. If a text edit is used, anchor on the *whole block* being
   replaced, never on a single line that has siblings.
2. After the edit, assert the resulting **tree**: the keys the consumer reads
   are present, at the path it reads them from. `assert cfg['a']['b']['c']` is
   the test; "it lints" is not.
3. A renderer that resolves missing values to empty rather than erroring cannot
   be used as a validator. Helm, envsubst, and most templating engines are in
   this class.

The upstream discipline: **a gate that cannot fail is not a gate.** `helm lint`
passing told us nothing about this defect, and reporting it as verification is
what let a broken chart reach an immutable registry version.
