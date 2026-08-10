# Agent Seat profile: X11 target-scoped text transfer v1

Identifier: `agent-seat.x11-text-transfer.v1`

Status: experimental repository profile, 2026-08-10. Supported binding: local
JSON wire revision 9. Backend atom: `x11_ewmh`. Assurance atom: `tier0`.
Required base profile: `agent-seat.x11-ewmh-core.v1` with
`observe_structure`.

## Authority and grant

This profile implements `text.insert` under the separate `text_transfer`
capability and advertises `text_transfer`. `input_keyboard` never implies the
grant. The operation needs the current volatile seat, but this profile does
not advertise physical-activity observation or physical-user priority.

The provider gains request-local authority to replace the X11 `CLIPBOARD`
owner and to send one balanced Control+V action. Replacing the selection loses
the prior owner; the provider MUST NOT read or pretend to restore its data. A
clipboard manager can request and retain offered text. Operator policy and UI
MUST disclose both effects beside this grant.

## Request and bounds

One `text.insert` request contains an exact fresh target and nonempty UTF-8 of
at most 32,768 bytes and 16,384 Unicode scalars. Newline and tab are the only
control characters. The target or one of its descendants MUST already own
X11 input focus; the provider MUST NOT force focus.

The provider MUST refresh scope, generation, and focus and recheck the same
enabled seat generation before it claims the selection and before it sends
the paste command. The command is resolved through the current XKB map and
sent as one complete Control+V action with balanced releases.

## Selection service

The reference realization creates one owner window for one request. It serves
only:

- `TARGETS`, returning the same finite supported atom list;
- `UTF8_STRING`;
- `text/plain;charset=utf-8`; and
- `text/plain`.

Before serving a request, it repeats target, focus, seat, and ownership checks.
It queries X-Resource 1.2 `CLIENT_XID` for the target and requestor and serves
text only when both resources belong to the same X11 client. A different
client receives a refusal `SelectionNotify` with no property. Missing or
ambiguous identity evidence fails closed. Same-client child and helper windows
are admitted; a helper on another X connection is deliberately unsupported.

A supported text request receives one complete property replacement followed
by `SelectionNotify` and X synchronization. The provider performs no partial
or incremental transfer. After the first complete text response, the reference
provider serves follow-up verified requests until 250 milliseconds pass
without another text delivery. It handles at most 32 selection requests and
256 X11 events for one transfer and waits no longer than two seconds. Bound
excess, selection, seat, target, or focus loss stops the request. Cleanup
clears `CLIPBOARD` only while the request-local owner still owns it, then
destroys that owner window.

## Qualified result

The result reports exact requested and delivered byte counts plus:

- `delivered` only after the verified requestor property and notification are
  synchronized;
- `offered` when the paste action was sent but no supported request arrived by
  the deadline; or
- `interrupted` when required evidence was lost.

Delivered bytes are either the complete requested count or zero. Delivery is
not evidence that an application inserted, displayed, retained, or understood
the text.

## Required evidence

A conforming implementation MUST provide public, reproducible evidence for:

1. exact non-ASCII multiline bytes after `TARGETS` negotiation;
2. refusal of a requestor from a different X11 client without disclosing text;
3. displacement of an existing selection owner without reading it;
4. terminal interruption on selection loss without clearing the later owner;
5. bounded timeout and request-local cleanup when no text is requested;
6. request byte/scalar/control bounds and all-or-nothing result validation;
7. separate grant enforcement and disabled-seat/focus/freshness refusal; and
8. absence of any wire or companion operation that reads a selection.

The reference suite also requires the same requestor to read the complete text
twice with a delay between requests. This guards the bounded post-delivery
service window used by consumers that prefetch when selection ownership
changes.

## Prohibited claims

This profile is not a clipboard API, clipboard restoration, arbitrary target
delivery, an accessibility insertion method, application acceptance, a
content-retention guarantee, physical-user priority, or Tier 1 authority.
