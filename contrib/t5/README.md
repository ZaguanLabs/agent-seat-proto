# T5 participant probes

`harness-authority-probe.rs` is dependency-free safe Rust for the external-
harness fixtures in
[`docs/t5-participation-contract.md`](../../docs/t5-participation-contract.md).
It is a hostile probe source, not an installed Agent Seat component, setuid
helper, sandbox, or conformance result.

Build it with the repository's supported Rust toolchain:

```sh
rustc --edition=2024 -D warnings \
  contrib/t5/harness-authority-probe.rs \
  -o target/t5-harness-authority-probe
```

The participant's trusted controller selects one event device the launcher
identity can open, the uinput node, live Xauthority file, provider/broker/user-
manager sockets, a visible parent PID, and the live X11 display number. It runs
the exact probe once outside the claimed boundary with `--expect baseline` and
once through every production harness/child launch path with
`--expect confined`.

Baseline succeeds only when all ten direct authorities are actually reachable.
Confined succeeds only when all ten are denied and no input descriptor was
inherited. The fixed `systemd-run --user --wait --collect` attempt uses only
`/usr/bin/true`; it proves whether the harness can escape through the user
manager and leaves no persistent unit.

Output is one closed JSON object of Boolean reachability facts and a verdict.
It contains no supplied path, PID, display number, socket payload, input event,
cookie, environment value, or process metadata. Exit status is 0 for the
selected expectation, 1 for an observed authority mismatch, and 2 when the
probe itself cannot produce complete evidence.

The probe intentionally does not create the boundary or test MCP semantics.
The participant must still run `harness.mcp-round-trip`, repeat the probe
through every supported child path, inspect all inherited descriptors, and
submit the complete review bundle. A passing local invocation alone is not the
harness gate.
