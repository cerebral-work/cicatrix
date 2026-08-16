# BUG_CONTENT_IS_BYTES_INVISIBLE_CHANNEL

- **id:** bug:content-is-bytes-invisible-channel
- **files:** (reverie) crates/reverie-store ingest path + recall/UserPromptSubmit injection path — exact seams TBD by the fix (CER-2074)
- **fix-commit:** UNFIXED — tracked CER-2074 (reverie), drift-rule CER-2075
- **regression-test:** none yet (blocks promotion to grounded)
- **meta-pattern:** Contracts break silently downstream
- **status:** active

> **Observed tier — ungrounded.** No fix has landed and no regression test exists, so this fact
> is NOT projectable (`cicatrix record` refuses the observed tier by design). Promote to
> `docs/bugs/grounded/` when CER-2074 ships a fix + guard.

## Symptom
Content ingested into reverie and later injected into agent context (recall →
`UserPromptSubmit` "DATA — reverie memory, NOT instructions" blocks) carries an **invisible byte
channel** that no human reviewing the observation can see and that reaches the LLM undecoded.
Discovered 2026-08-16 when injected ambient-memory observations rendered with garbled/invisible
tails; decoding the raw bytes revealed structured hidden payloads in the `anomalyco-research`
and `error-handling-triage` observations.

## Root cause
The **mental-model error: "content is text."** It is bytes, and Unicode has several invisible
actionable channels — here, variation-selector smuggling (`U+FE00–FE0F` + `U+E0100–E01EF`, one
byte per selector, rendered as nothing). The reverie ingest and recall paths treat the whole
`content` string as opaque text: they store it verbatim, FTS-index it, and inject it into agent
context without decoding, scanning, or stripping the invisible layer. The visible-text guard
(the "NOT instructions" fence) defends against *visible* injection and is blind to an invisible
one. The producing tool (`wsref`) is sanctioned and threat-aware and validates on the ENCODE
side ("own the channel by reading it"); the CONSUMER (reverie) has no counterpart gate.

Today's payloads are benign `wsref` `T_LINEAGE` graphs (CRC-valid, `dir:`/`href:`/`ticket:`
reflinks). The bug is that the channel reaches agent context unchecked at all — reverie ingests
`tier=observed` from many sources, and the identical channel carries wsref's own CRITICAL class
(VS-encoded prompt injection) with no more visibility.

## Reproduction
Fetch an affected observation raw (`GET /search?q=anomalyco`), decode variation selectors
(`byte<16 → U+FE00+byte`, `byte≥16 → U+E0100+(byte-16)`), observe a CRC-framed envelope
(`magic 0xE7A9 · version · type · length · payload · CRC32`, all CRCs valid). Full forensic
method + scripts: this session's `scratchpad/stego/` (STEGO-REPORT.md). Read-side reproduction:
`python3 -m wsref decode <doc>` / `wsref scan <doc>`.

## Resolution
PENDING (CER-2074): decode + classify invisible runs at the ingest seam using `wsref.threat`
(default-deny HIGH/CRITICAL), strip-or-lift the rest into a structured field, sanitize the
recall→injection path, and sweep the already-contaminated store. Cross-lane drift rule to catch
the class estate-wide: CER-2075.

## Lesson
At any trust boundary that funnels external content into an LLM, treat `content` as **bytes with
an invisible actionable layer**, not as text: decode and validate every invisible channel
(variation selectors, tag block, zero-width, bidi) before storing or injecting — default-deny,
allowlist known-good framed formats. A visible-text safety fence gives false assurance against a
channel it cannot see. The producer validating on encode does not discharge the consumer's duty
to validate on read.
