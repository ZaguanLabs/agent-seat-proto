# Agent Seat profile: standalone X11 Tier 0.5 input v2

Identifier: `agent-seat.x11-tier0.5-input.v2`

Status: experimental repository profile, 2026-08-09. Supported binding: local
JSON wire revision 7. Backend atom: `x11_ewmh`. Assurance atom: `tier0`.
Required base profile: `agent-seat.x11-ewmh-core.v1` with
`observe_structure`.

## 1. Relationship to version 1

This profile incorporates every authority boundary, volatile-seat rule,
pointer and text action requirement, result qualification, resource bound,
negative-authority requirement, hostile fixture, race disclosure, and
prohibited claim from
[`agent-seat.x11-tier0.5-input.v1`](x11-tier0.5-input-v1.md).

Version 2 adds only the revision-7 `keyboard.key` action described below.
Version 1 remains the immutable profile for wire revisions 5 and 6. A version-2
implementation MUST satisfy the version-1 requirements; a version-1
implementation does not thereby implement shortcuts.

## 2. Key-action claim

`keyboard.key` sends one complete, finite, layout-aware key action to a fresh
scoped target that already owns X11 keyboard focus. It uses the existing
`input_keyboard` grant, `input_injection` feature, volatile seat generation,
action-local server grab, XKB inspection, qualified `input` reply, and
reverse-order release cleanup. It adds no authority and no dependency.

One call contains one typed main key and zero to four unique momentary
modifiers. Modifiers MUST be in canonical `control`, `alt`, `shift`, `super`
order. The main-key registry is:

- Backspace, Delete, Enter/Return, Escape, Tab, Space, and Insert;
- Home, End, Page Up, Page Down, and the four arrow keys;
- lowercase `a` through `z` and digits `0` through `9`; and
- F1 through F12.

The X11 realization maps Page Up and Page Down to the standard `Prior` and
`Next` keysyms. `alt` selects an Alt keysym and never treats AltGr/ISO Level 3
as Alt. Letters and digits identify symbols in the effective layout, not fixed
physical rows. A symbol absent from the current group is refused rather than
approximated with a keyboard position.

## 3. Required ordering

For one key action the provider MUST:

1. verify the session grant and admitted seat generation;
2. acquire a short X server grab;
3. refresh and revalidate target identity, generation, and actual focus;
4. read complete live XKB state, types, symbols, and modifier mappings;
5. resolve the main symbol and every modifier before emitting any event;
6. recheck the volatile seat generation;
7. press required layout modifiers and requested modifiers, press and release
   the main key, release modifiers in reverse order, and synchronize X11;
8. release the server; and
9. recheck the seat before reporting.

Failure before step 7 emits no key. A failure after a press triggers bounded
best-effort release cleanup and MUST NOT be reported as a completed action.
Success reports `requested = 1`, `completed = 1`, and `queued`; a concurrent
post-send seat change may conservatively report `interrupted`.

## 4. Explicit exclusions

The profile exposes no backend keycode, arbitrary keysym, raw XKB mask,
layout/group mutation, compose or IME sequence, paste protocol, forced focus,
key hold, repeat, macro, multi-action sequence, or application-acceptance
claim. It does not add physical-activity observation or close the ordinary-X11
overlap race documented by version 1.

The finite operation can express conventional shortcuts such as Control+L,
Control+F, Control+W, Alt+Left, Alt+Right, Page Up, and Page Down only when the
live layout and focus evidence proves every component. Browser back/forward
media keysyms are not part of this version.

## 5. Additional conformance fixtures

In addition to all version-1 fixtures, an implementation claiming version 2
MUST provide public process-boundary evidence for:

- unmodified Page Down as one balanced key pair;
- Control+L as balanced modifier and main-key pairs;
- focus loss before resolution producing no event;
- a missing main symbol or modifier producing no event;
- duplicate, noncanonical, over-bound, and unknown modifiers being rejected;
- seat disable before emission producing no event and after emission producing
  only the qualified conservative result; and
- release cleanup after each injectable partial failure point.

Reference tests are evidence only for the tested build and environment. They
do not certify another implementation or strengthen the `tier0` assurance
atom.
