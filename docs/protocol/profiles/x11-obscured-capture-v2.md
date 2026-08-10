# X11 obscured-client capture profile v2

Status: experimental implementation in Agent Seat wire revision 8.

This profile incorporates every authority boundary, source restriction,
lifecycle rule, image bound, security meaning, failure condition, and
prohibited claim from
[`agent-seat.x11-obscured-capture.v1`](x11-obscured-capture-v1.md). Version 1
remains the immutable profile for wire revisions 6 and 7.

## Target-relative region call

Version 2 adds:

```text
capture.region({client, generation, x, y, width, height})
```

The call uses the existing `capture_obscured` grant and
`obscured_capture` feature. Coordinates are unsigned offsets from the target
client origin, not the frame, root, or output. The provider refreshes the
target under the server grab, requires the rectangle to fit its current
redirected pixmap, and passes only that rectangle to X11 `GetImage`. It does
not capture the full target and crop afterward.

The request and result rectangle is bounded by:

- `x + width` no greater than 2,048;
- `y + height` no greater than 2,048;
- width and height each from 1 through 1,024; and
- area at most 262,144 pixels.

The result repeats the exact target and rectangle, carries `format = "png"`,
and contains one canonical base64 eight-bit RGB PNG. All version-1 PNG,
response, visual, cleanup, enrollment, and target-owned-storage limits remain
in force. The MCP companion emits the data once as an `image/png` content
block and leaves only target, rectangle, and format in structured content.

## Security and efficiency meaning

A smaller request reduces transferred pixels and model image input; it does
not increase assurance. The region can still contain target-owned pixels that
the person cannot currently see. It remains separately granted and reveals no
root, output, cursor, decoration, or other-client pixels. Coordinates do not
name a UI element and no result proves that a later click reaches the pictured
control.

## Additional conformance fixtures

In addition to every version-1 fixture, a version-2 implementation MUST prove:

- exact region dimensions and known target-owned pixel values while covered;
- refusal when the rectangle extends beyond current target geometry;
- rejection beyond every side, extent, area, PNG, and frame bound; and
- region metadata in structured results without duplicated base64 data.

Reference evidence remains specific to the named build and environment.
