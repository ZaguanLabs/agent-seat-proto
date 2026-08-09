# Agent Seat Protocol

`agent-seat-proto` is the canonical Apache-2.0 project for a bounded protocol
between desktop providers and authority-free agent companions. The repository
is owned from its first commit by
[`ZaguanLabs`](https://github.com/ZaguanLabs).

E1 and the T0--T3 Tier 0 core are complete. Current source implements strict
Agent Seat wire revision 4, a generic MCP `2025-11-25` companion, and a standalone
provider with bounded EWMH observation, freshness-checked management, and
policy-controlled desktop-entry launch. The five deliverables are:

- `agent-seat-proto`: display-server-neutral wire types and framing only;
- `agent-seat-mcp`: a generic MCP translator with no policy authority;
- `agent-seat-x11`: a standalone Tier 0 provider for unmodified EWMH window
  managers such as Openbox; and
- `agent-seat-settings`: a human-facing policy editor with display-independent
  validation, inspection, and recovery commands plus a GTK 4 interface; and
- `agent-seat-activity-broker`: an experimental, separately confined Linux
  activity gate for the optional pointer-move profile.

The Tier 0 core provides bounded observation, supported EWMH management,
and controlled desktop-entry launch. Capture, input, and accessibility are
separate optional profiles and are not core-release promises.

Revision 4 contains one experimental `pointer.move` operation. It remains
unavailable without an explicitly configured broker, exact inherited devices,
and a separately trusted session/lock eligibility source. The repository ships
an explicit review, install, arm, stop, and purge workflow, but does not claim a
supported generic-Openbox deployment.

Administrators can review the current `seat0` device candidate without reading
input events or changing the system:

```sh
agent-seat-activity-enroll inspect --seat seat0
```

They can also render that exact candidate as four inert systemd units, a strict
complete input-class manifest, a bounded relevant-device identity record, and
a plain-text review record in a new private directory:

```sh
agent-seat-activity-enroll render \
  --uid "$(id -u)" \
  --session "$XDG_SESSION_ID" \
  --output /absolute/path/to/new-review-directory
```

Immediately before any later administrative review, regenerate the current
seat candidate and require the directory to match it exactly:

```sh
agent-seat-activity-enroll verify \
  --uid "$(id -u)" \
  --session "$XDG_SESSION_ID" \
  --bundle /absolute/path/to/review-directory
```

All three operations are unprivileged. Verification requires the exact seven
direct files, contents, owner, private modes, UID, session, and current device
set. None of these commands installs, enables, or starts the broker, enables
input policy, or makes the experimental profile supported.

After installing the optional broker, guard, and enrollment binaries at their
packaged `/usr/bin` paths, an administrator can explicitly publish the reviewed
bundle without starting or enabling anything:

```sh
sudo agent-seat-activity-enroll install \
  --uid "$(id -u)" \
  --session "$XDG_SESSION_ID" \
  --bundle /absolute/path/to/review-directory \
  --confirm-install
```

The administrator may then perform one non-persistent arm cycle, stop it, or
remove the exact UID-bound enrollment and units:

```sh
sudo agent-seat-activity-enroll arm \
  --uid "$(id -u)" --session "$XDG_SESSION_ID" --confirm-arm
sudo agent-seat-activity-enroll stop --uid "$(id -u)"
sudo agent-seat-activity-enroll purge --uid "$(id -u)" --confirm-purge
```

`arm` does not enable startup, and stopped brokers do not automatically rearm.
These commands exist to exercise the remaining security gates; they do not yet
make generic Openbox input a supported deployment.

The optional package must install `contrib/sysusers.d/agent-seat.conf` as
`/usr/lib/sysusers.d/agent-seat.conf` and run `systemd-sysusers` before arming.
This creates the locked `agent-seat-guard` system identity needed for stable
system-bus authentication. It has no home, login shell, supplementary group,
or input-device access. The evdev broker itself remains a `DynamicUser=`;
the provider pins UID 0 because PID 1 owns the socket-activated listener.

Inspection and session eligibility monitoring do not require root or group
membership. Physical event nodes commonly remain `root:input` mode 0660. The
normal design does not add the desktop user or broker account to `input`:
systemd opens only the reviewed nodes and passes those exact read-only
descriptors to the unprivileged broker. The unit's strict device filter names
those same nodes read-only so systemd may perform the open; the dynamic broker
still cannot open them through ordinary filesystem permissions or see host
devices in its private `/dev`. See the permission model in
[`docs/t5-broker-deployment.md`](docs/t5-broker-deployment.md).

The eligibility guard compares the complete live input-event class mapping to
the reviewed manifest after subscribing to bounded kernel device-lifecycle
notifications, then stops on any later input-subsystem change without reading
input events. Manifest ownership must match the authenticated service-manager
peer; the production profile fixes that peer and the installed manifest to
root. The new root-only file and lifecycle transactions have passed isolated
fixture tests, and both service profiles pass an explicit rootless hostile
sandbox probe on systemd 258. The hardened guard profile also passes a live handshake
with the current complete sysfs manifest, kernel uevent subscription, and real
logind session. Root-owned installed startup now reaches broker `Ready` with
the intended identities and bounds. A same-host inspection confirms the
production identities, zero capabilities, no supplementary groups,
`NoNewPrivileges`, seccomp, and private device views. A repeatable hostile test
also passes under the system manager with the production identity models. An
exact installed-unit fixture, actual hotplug, and trusted lock-transition
behavior remain open approval gates.

The private identity record binds each relevant event device to its canonical
sysfs path, udev physical path, classes, selected hardware IDs, complete kernel
event-capability bitmaps, and a short serial when one exists. Devices without a
serial remain topology-only for hardware identity, but their complete activity
coverage is bound by topology, capabilities, and the separately verified full
event set. An indistinguishable clone is therefore coverage-equivalent; this is
not a claim of physical-device attestation.

The current provider target is a local Linux X11 session. Other Unix peer
credential mechanisms and non-X11 backends are not yet supported.

## First run

Run the provider once from your X11 desktop session:

```sh
agent-seat-x11
```

If no configuration exists, the command creates a private, extensively
commented template at `$XDG_CONFIG_HOME/agent-seat/config.toml`, falling back
to `$HOME/.config/agent-seat/config.toml`, and exits without connecting to X11.
The template contains the current UID, explains every setting and capability,
and remains disabled until the user explicitly changes `enabled = false` to
`enabled = true`.

After reviewing the policy, validate and start it:

```sh
agent-seat-x11 --check-config
agent-seat-x11
```

The provider runs in the foreground. Add `agent-seat-x11 &` to Openbox
autostart after validating the policy. See [the provider guide](docs/provider.md)
for the complete configuration and security model. `agent-seat-x11 --help`
also describes the first-run flow and command-line options.

The Settings command can inspect and recover policy without a display:

```sh
agent-seat-settings --check
agent-seat-settings --print
agent-seat-settings --restore-previous
```

Run `agent-seat-settings` with no command to open its GTK interface. The
complete interface edits activation, capability grants, visible-window scope,
the bounded XDG launch catalog, and resource limits. It validates and shows an
exact policy diff before an atomic save, retains a private recovery policy, and
distinguishes saved policy from best-effort active-provider evidence. See
[the Settings guide](docs/settings.md) for the complete first-run and recovery
workflow.

The first supported source release is product tag `v0.1.0`. Its component
versions are `agent-seat-proto` 0.1.1, `agent-seat-mcp` 0.1.1, and
`agent-seat-x11` 0.1.4; crate versions and the wire revision are intentionally
separate identities.

## Build

Rust 1.85 or newer is required. The repository pins its minimum toolchain for
the ordinary source gate. Building the Settings interface also requires GTK 4
development files (`libgtk-4-dev` on Debian-family systems):

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo doc --workspace --no-deps
```

`agent-seat-mcp` can initialize and list its static tools without a desktop.
Its first tool call resolves `--socket`, `AGENT_SEAT_SOCKET`, or the live
selection-bound X11 advertisement. The standalone provider answers
authenticated `seat_status`, bounded desktop snapshots, filtered event
subscriptions, supported EWMH management, and controlled XDG application
discovery and launch. The gated pointer-move profile is present in source but
is not part of the supported Tier 0 core.

The normative wire contract is [`docs/specification.md`](docs/specification.md),
the companion contract is [`docs/mcp.md`](docs/mcp.md), and provider setup is
[`docs/provider.md`](docs/provider.md). Settings usage is
[`docs/settings.md`](docs/settings.md). Optional-profile stop decisions are in
[`docs/optional-profiles.md`](docs/optional-profiles.md). The
implementation-independent standards direction is the repository's non-external
[`R0 pre-RFC draft`](docs/r0-protocol-rfc.md).

## Project policy

Contributions are Apache-2.0 under DCO 1.1 sign-off. Read
[`CONTRIBUTING.md`](CONTRIBUTING.md) and [`PROVENANCE.md`](PROVENANCE.md) before
submitting work. Security reports use GitHub private vulnerability reporting as
described in [`SECURITY.md`](SECURITY.md).

This project is independently authored. Nobox is prior art and a future
black-box compatibility target, not a source dependency. No Nobox code,
history, fixtures, schemas, or prose is imported here.
