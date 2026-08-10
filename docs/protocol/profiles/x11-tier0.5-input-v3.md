# Agent Seat profile: standalone X11 Tier 0.5 input v3

Identifier: `agent-seat.x11-tier0.5-input.v3`

Status: experimental repository profile, 2026-08-10. Supported binding: local
JSON wire revision 8. Backend atom: `x11_ewmh`. Assurance atom: `tier0`.
Required base profile: `agent-seat.x11-ewmh-core.v1` with
`observe_structure`.

## Relationship to version 2

This profile incorporates every authority boundary, volatile-seat rule,
pointer, text, and key-action requirement, qualified result, resource bound,
negative-authority requirement, hostile fixture, race disclosure, and
prohibited claim from
[`agent-seat.x11-tier0.5-input.v2`](x11-tier0.5-input-v2.md).

Version 3 adds only the revision-8 `keyboard.write` action. Version 2 remains
the immutable revision-7 profile. An implementation does not satisfy version
3 merely by accepting a larger `keyboard.type` value.

## Long-form text claim

`keyboard.write` exists for multiline text that exceeds the short
`keyboard.type` limit. One request contains:

- one fresh scoped target that already owns X11 input focus;
- nonempty UTF-8 text of at most 16,384 bytes;
- at most 4,096 Unicode scalar actions; and
- no control character except newline or tab.

Before emitting the first action, the provider MUST validate the complete
request and resolve every scalar exactly through the current effective XKB
group, types, levels, and bounded safe modifiers. One unavailable scalar
refuses the entire request without typing its prefix. The provider MUST NOT
substitute, normalize, transliterate, invoke a shell, use a clipboard, or
change the keyboard layout.

The reference provider's bounded English diagnostic identifies the first
unavailable scalar by one-based character position and Unicode code point.
That diagnostic is assistance for a person or agent, not protocol control
data, and does not change revision 8 semantics.

After preflight, each scalar remains an independently reportable action under
the version-2 action-local server-grab contract. The provider refreshes the
target and focus, rechecks the volatile seat, rereads live XKB evidence,
queues one balanced character action, synchronizes, releases the server, and
rechecks the seat. Lost focus, target, seat generation, or layout evidence may
therefore stop a partially completed write. The `input` result reports exact
`requested` and `completed` scalar counts plus `queued` or `interrupted`; it
never claims application acceptance.

## Companion projection and repetition aid

The MCP `keyboard_write` tool maps one-to-one to `keyboard.write`. Its
response-read deadline may be longer than ordinary calls but MUST remain
finite; the reference companion uses 120 seconds and restores its ordinary
ten-second deadline afterward.

The reference companion also offers up to 32 session-local named pointer
slots. This is an MCP convenience, not a new provider call or authority. A
slot stores exactly one `pointer.click` argument set and disappears on provider
failure, context release, or companion exit. Replay requires a freshly
observed generation and sends one ordinary `pointer.click`, so every version-2
provider check still applies. Slots do not
identify elements, verify workflow success, retain pixels, survive sessions,
or execute sequences. Agents should save only a click whose effect was
observed and should reobserve after UI or layout changes.

## Explicit exclusions

Version 3 adds no clipboard or selection ownership, paste, compose/IME,
arbitrary Unicode guarantee, forced focus, held key, unbounded input, macro,
automatic repetition, element identity, workflow recording, application
acceptance, physical-activity observation, or physical-user priority.

## Additional conformance fixtures

In addition to every version-2 fixture, an implementation claiming version 3
MUST provide public process-boundary evidence for:

- a multiline write containing more than 256 scalar actions;
- whole-request refusal before send when any scalar is unavailable;
- exact partial count after a deterministic seat, focus, target, or layout
  interruption;
- balanced press/release cleanup across the complete write;
- rejection beyond either the 4,096-action or 16-KiB bound; and
- restoration of any companion-specific extended transport deadline.

Reference tests establish only the tested build and environment. They do not
close the ordinary-X11 overlap race or strengthen `tier0` assurance.
