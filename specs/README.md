# Specs

This directory holds the specification registry. Every spec is one file.

| Spec | Title | Status | Priority |
|---|---|---|---|
| [0001](0001-cicatrix-regression-db-provider.md) | Cicatrix regression db provider | in-progress | high |

## How to add a spec

1. Take the next free number. Use four digits. Example: `0002`.
2. Name the file `NNNN-kebab-slug.md`. Make the H1 read `# NNNN-kebab-slug`.
3. Copy the fenced template block from `standards/ears/spec-template.md`.
4. Keep the sections in this order: Requirements, Design, Tasks, Verification, Notes.
5. In Requirements, express acceptance criteria using EARS syntax with the keyword `shall`.
6. Put runnable commands in a fenced block under Verification.
7. Keep the file under 300 lines. Split a long spec into sub-spec files.
8. Add one row to the table above, with a relative link.
9. Run `cargo xtask spec-check -j 2`. It must pass.
