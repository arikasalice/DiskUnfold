# F15 Windows caption and Linux header bar (branch codex/f15-titlebar-win-linux)

Evidence screenshots are in `docs/handoff/2026-11-13/titlebar/win-linux/` of the main checkout.

## Linux: unstyled page

Root cause: the GTK scheme handler set the content type on the response and then installed its own
header set (`set_http_headers`) with only Content-Security-Policy and `X-Content-Type-Options: nosniff`.
The stylesheet arrived without a media type. Under `nosniff` WebKit discarded it: the `<link>` had a
sheet object with 0 rules (the request log showed `text/css; charset=utf-8`, 47866 bytes, served).
The CSP, the base URL and the sandbox switch were not involved (removing the CSP, adding CORS headers
and a different sandbox state changed nothing; adding Content-Type to the header set fixed it).
`0.1.0` has identical code and the same unstyled page. Windows and macOS already put Content-Type in their
header sets. The fix is `response_headers` in `platform-os/src/webview/contract.rs`; the Linux smoke test now
loads a stylesheet and fails without the fix.

## Linux: header bar

The window carries a GTK header bar with the app name as title and the screen as subtitle, taken from
`document.title` ("DiskUnfold \u2014 <screen>"). GTK and the system theme draw it, so it follows the desktop
button layout and light/dark setting that the page already follows.

## Windows: caption

The OS caption stays and follows the page: caption, text and border colors use `--du-page`, `--du-text`
and `--du-border` (a test compares them with `tokens.css`), the dark-mode attribute follows the app theme,
high contrast keeps the system colors, and the title follows `document.title`.

A 40 px web bar with the system caption buttons is not possible with a window-hosted WebView2:
DWM draws the system buttons only inside the non-client caption area and clips them to its height.
When the client area covers the caption (`WM_NCCALCSIZE` keeping the top), the buttons disappear;
with 8 px or 20 px of non-client height they appear clipped (`win-exp-*.png`, code in `winframe-experiment/`).
A page-drawn bar would need page-drawn caption buttons, which loses the system's snap layout flyout
and the buttons' UI Automation. Options for a later decision are listed in the PR.

## Open

- The main workspace screen sets no screen label, so its title is "DiskUnfold" (startup and closing set one).
- The Linux startup screen says "this Mac" and "Privacy & Security Settings".
