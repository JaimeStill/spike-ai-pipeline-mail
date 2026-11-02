# Standards

The judgement calls the check can't enforce, one line each, for the standards-reviewer. What
`mise run check` enforces is never restated here.

The architecture pages that apply, in the architecture repository:

- `principles/minimal-footprint.md`
- `principles/validation-first.md`
- `principles/rolling-currency.md`

## The CLI

- The CLI is used like any system tool: its output contract (the JSON model) is versioned with
  its tags, and a change to it is a CHANGELOG entry.
- A dependency joins only when it is actively maintained and freely licensed; an unmaintained or
  paid project is never adopted.
- No clippy lint is allowed without a comment naming why it is safe.
