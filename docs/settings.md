# Agent Seat Settings

`agent-seat-settings` is the graphical policy editor for `agent-seat-x11`.
It edits the same strict TOML policy as the provider and uses the provider's
own parser, application catalog, and atomic replacement code. It does not
start, stop, signal, or reload a provider and does not connect to its socket.

## First run

Open the editor from a terminal inside the desktop session:

```sh
agent-seat-settings
```

When the default policy is missing, Settings creates the same private,
commented, disabled policy as the provider's first run. No permissions become
active until the policy is explicitly enabled, saved, and loaded by a provider.
An explicit policy can be opened with an absolute path:

```sh
agent-seat-settings --config /absolute/path/to/config.toml
```

Source installations may install
`contrib/org.zaguanlabs.AgentSeat.Settings.desktop` in an XDG applications
directory to expose **Agent Seat Settings** in the desktop application menu.
For a per-user source installation:

```sh
install -Dm644 contrib/org.zaguanlabs.AgentSeat.Settings.desktop \
  "${XDG_DATA_HOME:-$HOME/.local/share}/applications/org.zaguanlabs.AgentSeat.Settings.desktop"
```

## Reading the state rail

The rail at the top keeps three different facts visible:

- **Saved** describes the last policy read from disk.
- **Draft** says whether controls differ from that saved policy.
- **Active** reports best-effort, lock-held evidence published by current
  `agent-seat-x11` processes for the same policy path.

`Matches saved` means the reporting provider loaded the exact saved bytes.
`Different` means the file was changed after that provider started; restart
the named provider process after reviewing and saving. `Not reported` does not
prove that no provider is running because older builds do not publish this
evidence. Active-state reporting grants no authority and is not a same-user
security boundary.

## Editing safely

The pages divide one policy into bounded tasks:

- **Overview** explicitly enables or disables the policy and exposes the exact
  saved and recovery paths.
- **Access** manages all observation, management, and launch capability atoms.
  Dependencies are stated beside each control and are never enabled silently.
- **Visible windows** selects no clients, the current workspace, or all
  workspaces and independently gates title text.
- **Applications** selects deny, allow-list, or allow-installed mode and shows
  the provider's bounded XDG catalog. Canonical desktop IDs and user-entry
  status remain visible. Search only filters this in-memory catalog. Each mode
  retains its bounded selections in the saved policy, while the provider
  consults only the entries applicable to the active mode.
- **Limits** controls the published session, request, and I/O timeout bounds.
- **Review** shows the exact removed and added policy lines and the result of
  strict candidate validation.

Invalid dependency combinations are refused and leave the draft unchanged.
`Save changes` becomes available only for a changed, valid draft. Its
confirmation explains the known restart consequence before the atomic write.
If another writer changed the file after Settings opened it, saving refuses
the stale draft and asks the user to reload rather than overwriting it.

Every successful replacement keeps the displaced private policy at
`config.toml.previous`. **Restore previous policy** validates both files and
atomically exchanges them. Reloading, discarding, restoring, and closing with
unsaved work require explicit decisions where draft data could be lost.

## Terminal recovery

These commands do not initialize GTK and work without X11 or Wayland:

```sh
agent-seat-settings --check
agent-seat-settings --print
agent-seat-settings --restore-previous
```

Add `--config /absolute/path/to/config.toml` before the command to operate on
an explicit policy. `--check` and `--print` require an existing policy and are
read-only. `--restore-previous` changes only the saved file; a running provider
continues using its startup policy until restarted.

## Current authority boundary

Agent Seat Tier 0 can observe scoped EWMH state, request supported EWMH window
management, and launch admitted desktop entries. It cannot capture the screen,
type, click, inject input, or use an accessibility tree. Settings deliberately
does not offer controls for those unsupported optional profiles.
