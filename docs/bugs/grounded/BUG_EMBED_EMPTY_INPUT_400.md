# BUG_EMBED_EMPTY_INPUT_400

- **id:** bug:embed-empty-input-400
- **files:** crates/reverie-store/src/embed.rs
- **fix-commit:** #609 (CER-914)
- **regression-test:** embed empty-input → zero-vector (not 400)
- **meta-pattern:** Type mismatches kill
- **status:** resolved

## Symptom
An embedding batch containing one or more empty-string inputs caused the **whole batch** to fail
with HTTP 400 — poisoning unrelated valid inputs in the same request.

## Root cause
The embedder treated "empty input" as an error condition rather than a representable value. The
mental-model error: conflating *absent* with *invalid*. An empty string is a legitimate input
whose correct embedding is a defined zero/degenerate vector, not a request-level failure.

## Reproduction
Submit a batch `["valid text", "", "more text"]` to the embed endpoint → entire batch 400s.

## Resolution
Map empty inputs to a zero vector and process them in-band; the batch succeeds and emits a
well-defined vector for the empty slot.

## Lesson
Validate at the seam and choose an explicit representation for the empty/degenerate case. A value
crossing a boundary in the wrong shape fails silently (or loudly, for the wrong reason) downstream.
