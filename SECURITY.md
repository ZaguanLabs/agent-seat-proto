# Security policy

## Reporting

Report vulnerabilities through this repository's GitHub **Report a
vulnerability** form. It creates a private security advisory visible to the
ZaguanLabs security maintainer. Do not open a public issue for an undisclosed
vulnerability.

Include the affected version, environment, smallest reproduction, impact, and
whether the report concerns protocol parsing, peer identity, policy, X11
behavior, or release infrastructure. Do not include unrelated personal or
desktop content.

The initial security owner is `kekePower`. Receipt is acknowledged as soon as
practical; assessment, fix coordination, disclosure timing, and credit are
handled in the private advisory.

## Supported versions

There is no supported release during E0. This section will list supported
versions before the first source release.

## Boundary

Same-user X11 clients are not isolated from one another. A standalone Agent
Seat provider constrains cooperative access through its own socket and policy;
it cannot prevent another process with the same X11 authority from bypassing
or spoofing X11-level state. Security claims must not exceed that boundary.
