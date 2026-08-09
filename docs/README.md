# Agent Seat documentation

Start with the project [README](../README.md) for installation and a product
overview. These guides cover normal configuration and operation:

- [Standalone X11 provider](provider.md) — first run, policy, launch admission,
  systemd integration, and troubleshooting.
- [Generic MCP companion](mcp.md) — MCP registration and the optional confined
  companion profile.
- [Settings](settings.md) — graphical and terminal configuration workflows.
- [Tier 0.5 volatile seat and input](tier-0.5-seat-gate.md) — explicit runtime
  enable/disable plus the bounded pointer, click, and focused-text claim.
- [Obscured-client capture](protocol/profiles/x11-obscured-capture-v1.md) —
  separately granted, bounded target-owned PNG capture and its limitations.
- [Compatibility](compatibility.md) — released and experimental combinations,
  evidence, and known limitations.

Technical material is grouped by purpose:

- [Design](design/README.md) — architecture, roadmap, UI design, and optional
  profile decisions.
- [Protocol](protocol/README.md) — wire contract, information model, registries,
  profiles, conformance formats, and pre-RFC work.
- [Security](security/README.md) — trust boundaries, threat models, and gated
  input/lock deployment studies.
- [Verification](verification/README.md) — milestone evidence and full-system
  participation contracts.

Documents describe only their stated assurance level. Experimental material is
not a promise that the corresponding capability is safe or available.
