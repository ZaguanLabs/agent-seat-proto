# X11 text-transfer browser verification

Date: 2026-08-10. Status: completed application-visible evidence for one
experimental provider patch, not a general browser, editor, or insertion
guarantee.

## Subject and environment

The subject was `agent-seat-x11` 0.1.31 using the unchanged
`agent-seat.x11-text-transfer.v1` profile and local JSON wire revision 9. The
live environment was Mageia 10, Linux 6.18.35, Xorg vendor release 12101023,
Openbox 3.6.1, Brave Browser Beta 151.1.94.104, and the Suno lyrics editor on
display `:0`. The user's Norwegian XKB layout remained unchanged.

The release candidate ran as the logged-in user in a transient systemd user
service with `PrivateDevices=yes` and `DevicePolicy=strict`. The installed
confined provider was stopped for the test; no root process, evdev access,
uinput access, clipboard-reading tool, XKB mutation, browser automation API,
or direct application script participated.

## Observed baseline

With installed `agent-seat-x11` 0.1.30, the freshly observed Brave client and
focused Suno editor accepted ordinary direct-XKB input. A one-byte
`text.insert` request for `x` reported `delivered` with one requested and one
delivered byte. Ordinary `keyboard.type` then queued `y`. After 500
milliseconds, a scoped capture visibly contained only `y`. The transfer had
therefore reached a verified Chromium X11 selection requestor but had not
become application text.

The provider implementation ended the selection transaction and destroyed its
request-local owner immediately after the first supported text response.
Chromium's public X11 clipboard implementation observes selection-owner
changes and prefetches types/plain text. This makes owner-change prefetch the
supported explanation for the first request; it is an inference from upstream
behavior, not something the wire reply itself proves.

## Candidate behavior

Version 0.1.31 retains selection ownership after the first complete response
until 250 milliseconds pass without another text delivery, while continuing
to enforce the original two-second, 32-request, 256-event, fresh-target,
focus, volatile-seat, ownership, and same-X-client limits.

The isolated Openbox/Xvfb fixture read the exact accented multiline payload,
waited 100 milliseconds, confirmed that the request-local owner remained, and
read the same complete bytes again. The selection-loss fixture received one
complete UTF-8 response before taking ownership during the quiet window; it
still reported `interrupted` and preserved the replacement owner. The
no-request fixture continued to report `offered` and clean up after the
two-second bound.

On the same live Brave/Suno field, `text.insert("x")` followed by direct
`keyboard.type("y")` visibly produced `xy` after 500 milliseconds. A separate
22-byte transfer visibly produced these two exact lines:

```text
sí, mañana
línea í
```

The field was selected, cleared, and captured afterward; the original
placeholder was visible, confirming removal of the diagnostic text. No form
was submitted and no keyboard layout was changed.

A long-form probe then transferred 120 numbered accented lines (3,479 UTF-8
bytes). The reply reported 3,479 requested and delivered bytes. After a
focused Control+End command, a scoped capture visibly showed the exact final
15 lines, numbered 106 through 120, including every displayed `í` and `ñ`.
The editor was again selected, cleared, and captured with its placeholder
visible.

## Limits

This evidence establishes application-visible insertion only for the named
Brave Beta build, Suno editor state, Openbox/Xorg environment, and short probe
payloads plus the one 3,479-byte probe. Only the visible tail of the long probe
was application-observed; unseen intermediate lines are supported by exact
selection delivery but were not independently read back from the editor. This
does not prove every payload size, every Chromium editor, other
browsers/toolkits, undo semantics, cursor placement, form submission, or
insertion merely from a future `delivered` reply. The reply therefore remains
qualified as selection delivery, not application acceptance. Clipboard
displacement, possible clipboard-manager retention, same-X-client scope, and
the lack of physical-user priority are unchanged.
