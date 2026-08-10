# Revision 8 interaction-efficiency verification

Status: local milestone evidence, 2026-08-10.

## Exact subject

- `agent-seat-proto` 0.1.6, local JSON wire revision 8;
- `agent-seat-mcp` 0.1.8, MCP `2025-11-25` and `2026-07-28` projections;
- `agent-seat-x11` 0.1.28, XKB/XTEST and Composite realization; and
- `agent-seat-settings` 0.1.9, broadened existing grant descriptions.

The claimed experimental profiles are
[`agent-seat.x11-tier0.5-input.v3`](../protocol/profiles/x11-tier0.5-input-v3.md)
and
[`agent-seat.x11-obscured-capture.v2`](../protocol/profiles/x11-obscured-capture-v2.md).

## Environment

The process tests ran on Linux in the repository's isolated Xvfb display with
released Openbox 3.6.1. Input used the X server's live XKB map and XTEST.
Capture used X Composite 0.4. MCP slot continuity used the real stdio companion
against a strict fake revision-8 pathname-Unix provider. No test connected to
the person's desktop.

## Direct observations

- A focused fresh target accepted one `keyboard.write` containing 300 scalar
  actions with 150 preserved newlines, exceeding the retained 256-action
  `keyboard.type` limit. The response reported 300 requested and completed,
  and observed X11 press/release multisets were balanced.
- Existing tests continued to refuse unfocused text before input and to report
  deterministic exact partial interruption when the volatile seat changed.
- While a differently colored override-redirect window covered the target, a
  64×32 region at client-relative `(32, 24)` decoded to the exact requested
  dimensions and the target's red pixels rather than the covering green
  pixels.
- A rectangle extending beyond the current target returned
  `invalid_argument` instead of clipping, reading another source, or falling
  back to full capture.
- A modern MCP context saved and listed one named click without a provider
  request. Replaying with a newly supplied generation produced exactly one
  typed `pointer.click` carrying that generation on the provider connection.
  The subsequent provider call and explicit context release succeeded in
  order.
- Closed-schema unit tests covered both new wire tools, all 22 legacy and 23
  modern tool definitions, region metadata without base64 duplication, slot
  name bounds, long-form action bounds, and exact capability selection.
- The repository quality gate passed locally:
  `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace --all-targets`, and
  `cargo doc --workspace --no-deps`.

## Limitations and inference boundary

The evidence proves only bounded wire handling and the observed reference
realizations. It does not prove that an application accepted typed text or a
click, that 4,096 actions complete within every application's timing behavior,
or that every Unicode scalar exists in every layout. Long-form input is not a
clipboard or IME path.

A pointer slot remembers coordinates and a fresh target generation, not an
element or workflow. It has no pixels, title, application identity, automatic
sequence, persistence, or success detector. Live provider checks can refuse a
replay, but they cannot prove that an application kept the same control at the
same coordinates.

Region capture remains separately granted obscured target storage. It does not
capture an output, cursor, decoration, or pixels painted before Composite
enrollment. None of these changes advertises `human_activity` or closes the
ordinary-X11 physical/agent overlap race.
