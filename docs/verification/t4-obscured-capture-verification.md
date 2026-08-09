# T4 obscured-client capture verification

Status: local experimental milestone record, 2026-08-09. This is reference
implementation evidence, not independent profile conformance or a release.

## Subject

- `agent-seat-proto` 0.1.4, wire revision 6
- `agent-seat-mcp` 0.1.5
- `agent-seat-x11` 0.1.26
- `agent-seat-settings` 0.1.7
- profile `agent-seat.x11-obscured-capture.v1`

## Environment

- Mageia Linux, kernel `6.18.35-desktop-1.mga10`, x86-64
- released Openbox 3.6.1
- Xvfb 21.1.23 with Composite 0.4 in an isolated display
- libXcomposite 0.4.6
- Rust and Cargo 1.85.0

No test used the person's live desktop. No package, service, VM, device access,
or privileged operation was added for this milestone.

## Observed evidence

The process-boundary fixture
`obscured_capture_returns_only_fresh_target_owned_pixels` starts isolated Xvfb
and released Openbox, starts the real provider with a separate capture grant,
and opens a real wire session. It observes a 320-by-180 client, thereby
enrolling it in provider-owned Composite automatic storage, then paints it red.
It places a green override-redirect window above the complete target and calls
`capture.obscured` with the exact observed handle and generation.

The fixture decodes the returned base64 and PNG, checks its RGB format and
dimensions, and observes red at the center rather than the green covering
window. It closes the session while leaving the client alive and independently
requires `NameWindowPixmap` to fail, showing that the provider's automatic
redirection was removed. A replacement session receives a new scoped handle;
after client destruction, capture through that handle returns
`no_such_client`.

Protocol and provider unit tests additionally observe:

- separate `capture_obscured` capability selection and its
  `observe_structure` dependency;
- canonical revision-6 bounds for dimensions, pixels, PNG/base64, and response
  frames;
- rejection of zero/over-area dimensions and non-PNG-shaped base64;
- explicit X11 byte-order and TrueColor-mask conversion; and
- MCP projection to one `image/png` block with no image-data duplicate in
  `structuredContent`.

The complete non-ignored provider lifecycle suite passed 24 tests, including
the capture fixture, with two unrelated explicit heavyweight input gates left
ignored by design. The exact repository source gates also passed:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo doc --workspace --no-deps
```

## Limitations

This evidence covers Xvfb Composite 0.4 and Openbox 3.6.1 only. It does not
establish behavior with a separate compositing manager, another server, another
visual class, another window manager, or another operating system.

Composite cannot reconstruct target pixels already obscured before the
provider enrolled the client. The verified promise starts with target painting
after enrollment. The capture excludes window-manager decorations, cursor,
root, and output content. It does not prove that a person saw the pixels,
perform OCR, identify UI controls, ground coordinates, or authorize input.

The profile remains experimental until an independent implementation and
independent black-box evidence satisfy the pre-RFC maturity rules.
