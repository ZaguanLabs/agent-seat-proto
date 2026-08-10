# Revision 8 input-diagnostics verification

Date: 2026-08-10

Status: locally verified corrective milestone; wire revision and profiles are
unchanged.

## Subject

- `agent-seat-proto` 0.1.6, wire revision 8;
- `agent-seat-mcp` 0.1.9, legacy MCP `2025-11-25` and modern MCP
  `2026-07-28` projections;
- `agent-seat-x11` 0.1.29, direct-XKB text realization; and
- `agent-seat-settings` 0.1.9, unchanged.

## Environment

- Linux 6.18.35-desktop-1.mga10;
- Rust and Cargo 1.85.0;
- Xvfb 21.1.24;
- released Openbox 3.6.1;
- `setxkbmap` 1.3.4; and
- installed XKB data 2.40.

## Observed behavior

The isolated Openbox lifecycle test applies the Norwegian XKB layout and first
retains the existing application-visible `https://slashdot.org` regression. It
then submits one 301-scalar `keyboard.write`: 300 directly resolvable ASCII
characters followed by `í` (U+00ED).

The provider returns `invalid_argument` before sending any prefix. Its bounded
English diagnostic identifies `keyboard character 301 (U+00ED)` and says not
to change the user's XKB layout. The terminal capture remains exactly equal to
the earlier URL, so the diagnostic does not weaken whole-request preflight.
The existing unmapped U+10FFFF fixture likewise identifies character 1 and
emits no input.

The MCP process test observes the compact no-XKB-mutation guardrail in the
legacy initialization result and modern discovery result. The published
`keyboard_write` tool description says that refusal identifies the first
unavailable scalar. Initialization, discovery, and tool listing remain
desktop-free and do not connect to a provider.

## Local gates

The repository's required commands passed in one local sequence:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo doc --workspace --no-deps
```

The ordinary workspace suite passed. Tests marked as explicit privileged,
systemd, exhaustive-layout, or live-session gates remained ignored by their
declared conditions and are not claimed by this record.

## Limitations

This milestone improves refusal and agent guidance; it does not make `í`, `ñ`,
or arbitrary Unicode typeable through a layout that lacks those direct
symbols. It does not add compose/IME driving, selection ownership, clipboard
access, application-acceptance evidence, or another capability. The
[Unicode text-transfer design](../design/text-transfer.md) remains candidate,
unimplemented, and subject to its approval gates.
