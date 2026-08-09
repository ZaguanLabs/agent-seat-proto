# Agent Seat profile: standalone X11 Tier 0.5 input v1

Identifier: `agent-seat.x11-tier0.5-input.v1`

Status: experimental repository profile, 2026-08-09. This profile records a
useful X11 input bridge without claiming physical-user priority, application
acceptance, or isolation from another same-user X11 client. It has only
reference-implementation evidence and is not yet provisional.

Owning specification: the Agent Seat repository pre-RFC. Supported binding:
local JSON wire revisions 5 and 6. Backend atom: `x11_ewmh`. Assurance atom:
`tier0`.
Required base profile: `agent-seat.x11-ewmh-core.v1` with
`observe_structure`.

## 1. Claim

This optional profile lets a standalone provider use XTEST for bounded pointer
movement, one complete pointer click, and focus-bound text. Every action is
bound to a fresh scoped target and an explicitly enabled, volatile provider
seat. Pointer destinations must still visibly belong to the target, and
keyboard focus must already belong to the target or one of its descendants.

The profile claims only the number of complete actions queued and synchronized
with X11. It does not claim that an application accepted, interpreted, or
rendered them. It does not detect physical activity and cannot prevent a
person's event from overlapping an agent action.

## 2. Supported surface

A session claiming this profile:

- advertises backend `x11_ewmh`, assurance `tier0`, and feature
  `input_injection`;
- does not advertise `human_activity` from this profile;
- grants `input_pointer` and `input_keyboard` separately;
- requires `observe_structure` with either input grant; and
- implements revision-5-or-6 `pointer.move`, `pointer.click`, and `keyboard.type`
  only for the granted subset.

The profile does not add wire values or change the base profile. Capture,
accessibility, arbitrary key events, shortcuts, held keys, dragging, multiple
clicks, scrolling, persistent coordinates, and emergency-stop claims are
outside version 1.

## 3. Actors and authority inventory

```text
requesting peer
    required: one authenticated Agent Seat connection and fresh target values
    prohibited: raw X11 identifiers or an independent input authority

standalone provider
    required: base-profile policy/scope authority, one X11 connection,
              live X11 target/focus evidence, XTEST, volatile seat state
    prohibited by this profile: evdev/uinput dependency, raw input reporting,
              forced focus, shell input, cached action sequences

foreign X server and window manager
    required: X11 object state, stacking/focus evidence, XTEST realization
    untrusted to establish physical-user priority or application acceptance

target and covering X11 clients
    untrusted: geometry, input shapes, focus children, mapping changes, timing

physical seat
    unobserved by this profile; may overlap every agent action
```

The provider's ordinary input path MUST work without a privileged service,
raw input-device descriptor, uinput descriptor, or membership in the group
owning input devices. A deployment may separately remove ambient device access
as defense in depth, but that does not strengthen this profile's assurance.

## 4. Volatile admission and lifecycle

Every new provider instance begins with its input seat disabled regardless of
saved policy. A disabled instance refuses Agent Seat session opening. Enabling
is an explicit local operator action, is not an MCP call, and is not persisted.

Each admitted session is bound to the enabled seat generation in which it
opened. Disable advances the generation, denies new sessions, and causes later
requests on old sessions to return a typed revocation. Re-enable never revives
an old session. Provider restart, X connection loss, advertisement loss, and
logout destroy the enabled state; a replacement provider begins disabled.

The operator control channel is deployment-specific and is not part of the
display-neutral wire. Same-UID authentication of that channel is practical
consent for a confined companion, not protection from a malicious process
already holding the desktop UID.

## 5. Common action boundary

Every independently reportable input action has this ordering:

1. verify the session grant and the admitted seat generation;
2. acquire a short X server grab;
3. refresh the scoped model and revalidate target identity and generation;
4. revalidate the operation-specific destination or focus evidence;
5. recheck the seat generation;
6. queue one complete bounded action through XTEST and synchronize;
7. release the X server; and
8. recheck the seat before reporting the terminal result.

No event is queued when a precondition fails. The server grab bounds target
changes caused by other X11 clients during steps 3 through 6; it does not stop
physical input or prove that foreign clients cannot race outside that interval.

## 6. Pointer actions

Pointer coordinates are unsigned offsets from the fresh target's current
client origin. Before movement, the provider verifies that the point is inside
the current client geometry and that the topmost effective X11 input ancestry
at the translated root coordinate belongs to that client or its window-manager
reparenting frame. Missing, malformed, over-bound, or ambiguous hit-test
evidence fails closed.

`pointer.move` queues one motion to the verified point. `pointer.click` first
queues that motion and then one complete primary, middle, or secondary button
press/release pair. A release failure triggers best-effort release cleanup and
is never reported as a completed action.

This profile does not define decoration clicks outside the client, coordinate
replay after a new observation, drag state, button holds, or multi-click timing.

## 7. Keyboard text

`keyboard.type` requires actual X11 input focus to be the fresh target or one
of its descendants. The provider never activates the client or forces focus as
a fallback.

Before sending the first character, the provider negotiates XKB and resolves
the complete request against the live effective group, key types, levels, and
available bounded momentary modifiers. An unavailable character or incomplete
XKB state refuses the request instead of guessing a keycode, changing the map
or group, using a shell, or pasting through another protocol. Newline maps to
Return and tab maps to Tab. Other control characters are invalid. Directly
mapped symbols may use bounded Shift, Level3, or Level5 modifier pairs, with
best-effort reverse-order release cleanup on failure. Compose/dead-key
sequences and input methods are outside this profile.

Each Unicode scalar is one independently reportable action. Target, focus, and
seat evidence are revalidated under a separate short server grab before every
character. The live XKB state and map are also re-read inside that grab. A
depressed or latched modifier makes the action unavailable; established
Caps/Shift/Num lock state may be honored only when the selected XKB type remains
unambiguous. These checks allow a request to stop with a conservative partial
count.

## 8. Result and interruption semantics

An input reply contains `completed`, `requested`, and one terminal value:

- `queued` means every requested action was queued and synchronized with X11;
- `interrupted` means required seat, target, focus, or backend evidence was
  lost before the complete request.

`completed` counts only complete independently reportable actions. It may be
smaller than `requested` only with `interrupted`. A post-send seat change may
produce `interrupted` even when the last action was already queued; this is a
conservative result, not an assertion that X11 withdrew the event.

A single pointer action may overlap the first physical event because this
profile has no separately trusted activity source. That race is part of the
public claim and MUST NOT be hidden behind a `human_activity` feature.

## 9. Bounds and denial of service

Revision-5 frame and target bounds apply. Text is nonempty, at most 1,024 UTF-8
bytes, and at most 256 Unicode scalar actions. Hit-test child, shape-rectangle,
and ancestry traversal counts are finite implementation limits. Server grabs
are action-local; a provider MUST NOT hold the server across the complete
multi-character request.

Unknown fields, unknown buttons, out-of-range coordinates, unsupported control
characters, over-bound text, absent keysyms, unavailable safe modifiers,
incomplete XKB state/maps, and incomplete X11 evidence are typed failures. Peer
input never raises a bound or selects raw backend keycodes or button numbers.

## 10. Black-box conformance fixtures

The required suite uses an isolated X server and a released window manager. It
observes public wire, process, socket, and X11 behavior rather than logs:

- `input.admission`: disabled opening refusal, explicit enable, separate input
  grants, disable/revoke/re-enable, and disabled restart;
- `input.pointer`: client-relative movement, all logical buttons with complete
  press/release observation, geometry bounds, and covering-window refusal;
- `input.keyboard`: non-focused refusal, descendant focus, XKB group/type/level
  resolution, Norwegian URL punctuation delivered exactly to an application,
  Return/Tab, unavailable-character no-send, and complete modifier/key cleanup;
- `input.interruption`: deterministic disable, target loss, and focus loss
  between character actions with exact conservative partial counts;
- `input.bounds`: strict schemas, byte/scalar limits, controls, coordinates,
  unknown fields, and bounded hostile X11 ancestry/shape inputs;
- `input.lifecycle`: provider, display, advertisement, and session-generation
  loss deny later input and replacement begins disabled; and
- `input.no-extra-authority`: functionality passes without raw input/uinput
  access, no raw event detail crosses the wire, focus is never forced, and
  `human_activity` remains unadvertised.

All seven fixture IDs are required. A report uses
[`agent-seat.conformance-report/1`](../conformance-report.md) or a later
registered format and states the same-user X11 and physical-overlap limitations.

## 11. Known limitations and prohibited claims

An implementation claiming this profile states all of the following:

- XTEST realization is not application acceptance;
- the volatile seat is operator consent and later-request revocation, not
  mandatory mediation or process cancellation;
- the profile has no trusted physical-activity, lock, presence, or priority
  evidence;
- same-user X11 clients may bypass Agent Seat or race its observations;
- text is limited to direct symbols in the current effective XKB group that are
  reachable with the current lock state and bounded momentary Shift, Level3, or
  Level5 modifiers; the provider does not switch groups or drive compose/IME;
- focus is required and never forced;
- pointer input is limited to the current visible target point; and
- matching the tool surface does not confer Tier 1 assurance.

## Non-normative reference evidence

The repository's revision-5 provider lifecycle tests exercise admission,
generation revocation, target-relative movement, all three logical clicks with
complete button pairs, covering-window refusal, actual-focus refusal,
lower/shifted/Return/Tab key pairs through the live XKB map, Norwegian URL
punctuation observed exactly by xterm, and unmapped-scalar no-send behavior. An
explicit installed-registry matrix requires every loadable layout/variant to
produce exact application-visible URL text or refuse before sending. A
pre-opened private control-channel fixture disables the
seat after the first observed key event and verifies the exact partial scalar
count against complete Shift/key pairs; it passed 20 consecutive local runs.
Protocol, MCP, Settings, and configuration tests exercise separate grants and
public bounds. A rootless process fixture establishes that the host user can
open uinput, then runs the real provider with a private `/dev` containing
neither `/dev/input` nor `/dev/uinput`; pointer click and keyboard text still
produce the expected public X11 events. The local
[verification record](../../verification/t0.5-input-verification.md) names the
exact environment and limitations.

The reference implementation now exercises every fixture behavior, including
the negative raw-device authority condition, but has not emitted one complete
implementation-independent conformance report. Independent evidence is also
required before the profile can become provisional.
