# Revision 9 text-transfer verification

Date: 2026-08-10. Status: experimental implementation evidence, not a general
X11, browser, toolkit, or application-acceptance claim.

## Subject

- `agent-seat-proto` 0.1.7, local JSON wire revision 9;
- `agent-seat-mcp` 0.1.10, legacy MCP `2025-11-25` and modern MCP
  `2026-07-28` projections;
- `agent-seat-x11` 0.1.30, `agent-seat.x11-text-transfer.v1`; and
- `agent-seat-settings` 0.1.10, separate grant and warning surface.

The local environment was Mageia 10 with Linux 6.18.35, Openbox 3.6.1,
Xvfb/Xorg 21.1.24, Rust 1.85.0, and x11rb 0.14. Tests used fresh isolated Xvfb
displays and released Openbox. They did not connect to or modify the person's
desktop, clipboard, applications, or provider instance.

## Observed evidence

The process-boundary X11 target used its own real window and selection request
rather than accepting provider logs as evidence. It already owned focus,
received the provider's balanced paste command, requested `TARGETS`, observed
`UTF8_STRING`, then requested and read the offered property. The bytes were
exactly:

```text
Canción íntima
Mi vida es mía, señor.
Mañana será mejor.
```

The provider reported `delivered` with requested and delivered byte counts
equal only after the property write, `SelectionNotify`, and X synchronization.
The request-local owner was gone and `CLIPBOARD` had no owner afterward.

The same deterministic fixture first installed a competing clipboard owner.
The provider displaced it without requesting its contents. While the real
target deliberately delayed its request, a separately connected hostile X11
client requested `UTF8_STRING`; it received `SelectionNotify` with no property
and no text. The legitimate same-client request then completed.

A second fixture replaced the provider as selection owner after receiving the
paste command. The wire result was `interrupted` with zero delivered bytes,
and provider cleanup preserved the replacement owner. A third target ignored
the paste command; the provider returned `offered` with zero delivered bytes
after its two-second bound and released ownership. An unfocused target was
refused before clipboard ownership changed.

Protocol tests cover the 32,768-byte and 16,384-scalar bounds, empty and
unsupported-control refusal, a maximum-size non-ASCII value, a byte-over-bound
value, the separate capability mapping, and the all-or-nothing relationship
between byte counts and terminal states. Provider policy and Settings tests
cover the separate dependency and warning. Both MCP eras publish one added
closed `text_insert` schema and compact instructions that distinguish
selection delivery from insertion and prohibit shell/browser clipboard
bypasses.

The ordinary repository gates passed for this exact source:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo doc --workspace --no-deps
```

## Negative authority and limitations

There is no selection-read wire call or MCP tool. The companion does not
connect to X11 for text transfer; it sends the bounded wire request only. The
provider code issues no selection conversion or property read for prior
clipboard contents. It exposes only request-local write ownership and the
finite selection service described by the profile.

The proof establishes exact selection-property delivery to the tested X11
client, not insertion, display, retention, cursor placement, undo behavior, or
acceptance by Brave, Chrome, Suno, or any other application. A clipboard
manager can retain the offered text. The old selection owner cannot be
restored. Same-client helper windows are supported; helpers on another X11
connection are refused. X-Resource 1.2 is mandatory and missing identity
evidence fails closed. Ordinary X11 still cannot provide physical-user
priority, and this profile does not advertise it.
