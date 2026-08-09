# X11 obscured-client capture profile v1

Status: experimental implementation in Agent Seat wire revisions 6 and 7.

This optional profile exposes one target-owned image without exposing the root
window or an output. It is deliberately separate from the
[X11/EWMH core profile](x11-ewmh-core-v1.md) and the
[Tier 0.5 input profile](x11-tier0.5-input-v2.md).

## Session contract

The provider advertises `obscured_capture` only when the authenticated session
is granted `capture_obscured`. That capability depends on
`observe_structure`. The only profile call is:

```text
capture.obscured({client, generation})
```

The target is the session's opaque client handle and exact observed
generation. Raw XIDs, output identities, root pixels, cursor pixels, and window
manager decorations never cross the wire.

## Source and lifecycle

For a capture-enabled observer, the provider selects X Composite automatic
redirection only for clients currently admitted by its configured scope. It
records which selections it owns, removes a selection when the client leaves
scope, and requests removal of every remaining selection when the session
ends. It never selects manual redirection and never unredirects another
client's selection.

When `capture.obscured` arrives, the provider grabs the X server, refreshes the
scope and target generation, requires the target to be viewable and already
enrolled, names its Composite pixmap, reads that pixmap, frees the name, and
releases the server. The image is encoded only after the grab is released.
Every cleanup failure makes the call fail closed.

Composite storage begins at enrollment. It cannot reconstruct target pixels
that were already obscured before enrollment. The profile therefore promises
the target-owned storage painted after enrollment, including such pixels later
hidden by other windows; it does not promise historical pre-enrollment content.

## Pixel and allocation bounds

Only Composite 0.2 or newer, a TrueColor visual, and a server-described 16-,
24-, or 32-bit ZPixmap storage format are accepted. Byte order, scanline
padding, bits per pixel, and visual color masks come from the X11 setup and are
validated before conversion. Indexed and DirectColor visuals are refused.

One result is an eight-bit RGB PNG carried as canonical base64:

- width: 1 through 2,048 pixels;
- height: 1 through 2,048 pixels;
- area: at most 2,073,600 pixels;
- decoded PNG: at most 7,340,032 bytes; and
- complete provider response: at most 12,582,912 bytes.

The result repeats the exact target and carries `format = "png"`. The MCP
companion converts it to one `image/png` content block and keeps only target,
dimension, and format metadata in `structuredContent`, avoiding a second copy
of the encoded image on its public response.

## Security meaning

This profile intentionally reveals content belonging to the granted target
that the person may not currently see because another window covers it. That
is why it requires a separate grant. It does not reveal another client's
pixels, capture the desktop, prove visual freshness beyond the named storage,
or claim that the person saw the image.

Missing extension support, target loss, stale generation, scope loss,
unviewable state, unsupported pixel evidence, excess dimensions or bytes, X11
read failure, and cleanup failure are errors. The implementation never falls
back to core `GetImage` on a client window, the root, or an output.

## Verification boundary

The isolated Openbox/Xvfb gate paints a known target after Composite enrollment,
covers it with a differently colored override-redirect window, decodes the PNG,
and requires the target color rather than the covering color. The same test
destroys the target and requires `no_such_client` on reuse of the stale handle.
Unit gates cover dimensions, PNG/base64 shape, X11 byte order, TrueColor mask
scaling, separate capability selection, and MCP image-block projection.

Passing those gates verifies this bounded profile only. It does not verify
output capture, pre-enrollment reconstruction, compositing-manager
interoperability on every X server, accessibility, OCR, or grounded clicking.
The [local milestone record](../../verification/t4-obscured-capture-verification.md)
names the exact reference subject, environment, observations, and limitations.
