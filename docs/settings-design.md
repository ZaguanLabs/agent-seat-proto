# Settings application design

## Product job

`agent-seat-settings` is a local policy editor for a person configuring Agent
Seat for the first time or reviewing an existing grant. Its single job is to
make the provider's exact saved policy understandable, editable, reviewable,
and recoverable without becoming another runtime authority.

The interface uses the person's language: access, visible windows,
applications, limits, review, and saved changes. Canonical capability atoms,
desktop IDs, paths, and TOML remain visible where they are useful evidence,
but they do not replace explanations.

## Toolkit decision

Use GTK 4 through the official safe `gtk4-rs` bindings, in a separate
`agent-seat-settings` crate. Target the GTK 4.0 API floor unless a later design
requirement justifies a higher feature gate. The bindings' documented minimum
Rust version is 1.83, below this workspace's Rust 1.85 baseline:
<https://gtk-rs.org/gtk4-rs/stable/latest/docs/gtk4/>.

GTK provides native text, focus, keyboard navigation, input methods, scaling,
theme integration, and accessibility semantics on X11 without implementing a
second widget system. The dependency belongs only to the Settings executable;
`agent-seat-x11`, `agent-seat-mcp`, and `agent-seat-proto` retain their current
runtime dependency boundaries.

Do not add libadwaita initially. Its preference widgets are useful, but the
additional platform dependency is not required for this bounded application.
Do not use a web view, an immediate-mode game/rendering UI, shell dialogs, or
raw X11 widgets. Those choices would add another runtime, weaken native
accessibility, fragment interaction, or create a disproportionate local UI
implementation.

The terminal-independent policy core remains usable without GTK or a display,
so recovery and process tests do not depend on graphical initialization.

## Visual direction

The application should feel like a calm instrument panel for a local trust
boundary, not a generic administration dashboard.

- `work surface` — `#EEF2F6`, a cool low-contrast canvas;
- `policy ink` — `#152333`, primary text and structural rules;
- `authority blue` — `#225EAA`, selected controls and deliberate grants;
- `verified green` — `#2D7D62`, exact validation and synchronized state;
- `attention amber` — `#9A5B00`, unsaved or restart-required state; and
- `refusal red` — `#A4312A`, invalid, conflicting, or destructive state.

Use the system sans-serif for body text and restrained semibold headings. Use
the system monospace face only for policy atoms, desktop IDs, paths, values,
and diffs. Do not ship a font dependency. GTK theme colors remain the fallback
for high-contrast or user-overridden themes.

The signature element is a persistent policy-state rail:

```text
SAVED ───────── DRAFT ───────── ACTIVE
valid            4 changes       restart required
config.toml       review          process 4812
```

It encodes the application’s central safety distinction. It is not a progress
indicator: each node independently reports known, unknown, matching,
different, or unavailable state. Color is always paired with text and an icon.

## Information architecture

At ordinary desktop widths, navigation and content use two columns while the
state rail stays above both. Narrow windows collapse navigation into a page
chooser; controls remain in the same order.

```text
┌─────────────────────────────────────────────────────────────────┐
│ Agent Seat policy                                  config path  │
│ SAVED ───────────── DRAFT ───────────── ACTIVE                   │
├────────────────┬────────────────────────────────────────────────┤
│ Overview       │ Page title                                     │
│ Access         │ Explanation                                    │
│ Visible windows│                                                │
│ Applications   │ Related controls in bounded groups             │
│ Limits         │                                                │
│ Review         │ Inline validation or dependency guidance       │
│                │                                                │
│                │                         Review changes          │
└────────────────┴────────────────────────────────────────────────┘
```

Pages have these responsibilities:

1. **Overview** shows the exact path, explicit enabled switch, saved-policy
   validity, recovery-file availability, and restart guidance.
2. **Access** groups observation, management, and launch capabilities. Every
   row explains its effect and names prerequisites. A prerequisite is never
   enabled silently; incomplete combinations remain visibly refused.
3. **Visible windows** controls client scope and the independent title-content
   switch, explaining that both policy and capability gates must allow titles.
4. **Applications** controls deny, allow-list, and allow-installed modes; a
   searchable bounded catalog shows localized names, canonical IDs, and a
   prominent user-entry badge. Allow and deny lists cannot overlap.
5. **Limits** exposes bounded session, request, and timeout values as advanced
   controls with their accepted ranges.
6. **Review** shows the exact before/after source diff, validation result, and
   one `Save changes` action. Saving uses the reviewed snapshot; conflicts ask
   the person to reload and never overwrite external changes.

## Interaction rules

- Loading, catalog discovery, rendering, and saving produce specific inline
  outcomes with a next action. Errors never collapse into “Something failed.”
- `Save changes` is disabled until the draft differs and exact validation
  succeeds. The confirmation names whether a provider restart is required.
- Closing with unsaved changes requires an explicit discard decision.
- Reloading after an external edit discards nothing without confirmation.
- Search filters the already bounded in-memory catalog; it does not launch an
  application or query a running provider.
- The application never starts, stops, signals, or reloads the provider.
- Every control is reachable by keyboard, focus remains visible, and no state
  relies on color alone. Custom motion is unnecessary; respect GTK's animation
  and reduced-motion settings.

## Design critique

A conventional preferences window with unrelated rows would hide the security
relationships and look interchangeable with any desktop settings panel. The
state rail and dependency-aware grouping instead encode facts unique to Agent
Seat. The palette is deliberately cool and operational, avoiding both alarmist
security styling and decorative “AI dashboard” gradients. The single visual
risk is making internal policy state permanently visible; that earns its space
because saved-versus-active confusion is a roadmap-level failure mode.
