# BUG_MD_PARSER_FENCE_BLINDNESS

- **id:** bug:md-parser-fence-blindness
- **files:** src/bug_md.rs
- **fix-commit:** CER-2078 (fix/cer-2078-parser-skip-code-fences)
- **regression-test:** `bug_md::tests::fenced_code_block_content_is_not_parsed_as_structure`
- **meta-pattern:** A text-anchored edit is not a structural edit
- **status:** resolved
- **scope:** src

## Symptom

`real_corpus_parses` went RED the moment a BugFact containing a fenced code block
landed in `docs/bugs/grounded/`. The failure:

```
slug not BUG_*: The break appears only when the API server validates the rendered object.
```

The slug was not a `BUG_*` value at all — it was a prose sentence lifted out of a
shell code block inside another bug's Reproduction section.

## Root cause

`bug_md::parse` is a line-by-line reader: `# ` sets the slug, `## ` opens a
section, `- **` records metadata, anything else is section prose. It did **not**
track fenced-code state, so every line inside a ` ``` ` block was still matched
against those prefixes. A code comment like `# The break appears …` matched
`strip_prefix("# ")` and **overwrote the slug** — the last `# ` line in the file
wins, and a Reproduction code block is near the end. `## ` and `- **` lines
inside fences were corrupted the same way (spurious sections, phantom metadata).

**The mental-model error:** treating a markdown document as a flat stream of
lines with unique structural prefixes, when it is a tree with quoted regions. A
` ``` ` fence is a context switch — its contents are *data*, not markup — and a
parser that pattern-matches line prefixes without honoring it cannot tell a real
`# heading` from a shell comment. This is the same class as the bug the corpus's
own `BUG_YAML_ANCHOR_REPARENTS_SIBLINGS` describes: a text-level operation
(prefix match / anchor insert) applied to something whose meaning is structural.

## Reproduction

Any grounded `BUG_*.md` whose prose embeds a fenced block with a `#`-prefixed
line reproduces it. Minimal:

```bash
# this comment is not a heading
helm template .
```

Before the fix, that ` # this comment is not a heading ` line became the parsed
slug. `cicatrix inject` / `project-meta` / `record` and the `real_corpus_parses`
guard all consumed the degenerate fact.

## Resolution

`parse` now tracks an `in_fence` flag toggled by any line whose first non-space
characters are ` ``` `. While inside a fence, the `# ` / `## ` / `- **`
structural arms are skipped and the line is preserved verbatim as section prose;
outside a fence, parsing is unchanged. Verified against the real triggering file
(`BUG_YAML_ANCHOR_REPARENTS_SIBLINGS`): its slug now resolves correctly and its
meta-pattern projects cleanly.

## Lesson

**A structural format needs a structural parser.** When a grammar has quoted or
fenced regions (code blocks, string literals, heredocs), the tokenizer must
model that state before matching anything else — line-prefix matching that
ignores fences will misread quoted data as markup. Corollary specific to this
tool: cicatrix stores regression memory *about code*, so its own corpus will
always contain code; a parser that cannot survive a code block in a bug doc
cannot do its one job. Test the parser against its own worst-case input (a bug
doc full of shell and YAML), not just clean prose.
