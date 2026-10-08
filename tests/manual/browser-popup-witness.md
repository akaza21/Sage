# Native popup Step A witness

Serve this directory on loopback (for example, from `tests/manual`, run
`python -m http.server 1421 --bind 127.0.0.1`) and open
`http://127.0.0.1:1421/browser-popup-witness.html` in a native Sage Browser tab.
Use a disposable Sage profile and an otherwise idle Project. Do not count a
normal browser tab or a Vite preview as native WebView2 proof.

1. Click **window.open child** once. Expect exactly one child tab directly after
   the opener, displaying `Child page: window-open`. Revisit the opener and
   repeat; each click should create one child, without duplicate replay tabs.
2. Click **target=_blank child**. Expect one adjacent child displaying
   `Child page: target-blank`.
3. In a fresh opener, click **Open child, then close this Sage tab immediately**
   and close the opener tab as quickly as possible. The child must not remain
   as an orphan if the native open was still in flight. Repeat to catch the
   timing window.
4. In a fresh opener, click **Open child, then reload opener immediately**.
   If a child opens, expect it directly after the opener. No duplicate should
   appear after the opener reloads or after switching away and back.

Record the Sage build SHA, OS/WebView2 version, profile path, pass/fail for each
case, and a capture of the tab strip plus child content. The child logs whether
`window.opener` exists; Step A does **not** promise a JavaScript `WindowProxy`,
so `false` is expected and is not a Step A failure. Do not claim OAuth popup
compatibility from this witness.

## Native result — 2026-10-08

Step A passed in a disposable native Sage build from `554482c6` (executable
SHA-256 `631506990A55961FB4E8842BBA3B7D11758E4E97D7CFB560602953B1C4929D43`).
Windows version was `10.0.26200`, WebView2 runtime `154.0.4258.62`, and the
window reported 96 DPI. The profile was
`C:\Sage\work\native-popup3\profile`; the earlier two-click run used
`C:\Sage\work\native-main-witness\profile`. All fixture clicks and tab
switches were CDP-dispatched into the actual native WebView2 targets, not
physical-pointer input. The loopback fixture contained no credentials.

| Case | Native result |
| --- | --- |
| `window.open` twice | Pass: each click created exactly one adjacent child tab; no replay duplicate. [Target/tab record](../../docs/work-items/CODEX-PARITY/evidence/browser-popup-step-a-window-open.json). |
| `target=_blank` | Pass: one adjacent child with `Child page: target-blank`. [Target/tab record](../../docs/work-items/CODEX-PARITY/evidence/browser-popup-step-a-blank.json). |
| Close opener immediately | Pass in two fresh-opener runs: closing the Sage opener tab immediately after the fixture click left zero browser tabs and zero native page targets. [Second-run record](../../docs/work-items/CODEX-PARITY/evidence/browser-popup-step-a-close-race.json). |
| Reload opener immediately | Pass: one adjacent `reload-race` child and no duplicate after switching away and back. [Initial](../../docs/work-items/CODEX-PARITY/evidence/browser-popup-step-a-reload.json) · [after switching](../../docs/work-items/CODEX-PARITY/evidence/browser-popup-step-a-switch-back.json) · [96-DPI native capture](../../docs/work-items/CODEX-PARITY/evidence/browser-popup-step-a-native-96dpi.png). |

The child reported `window.opener present: false` and the `window.open` fixture
reported a null handle, both within Step A's documented contract. This does not
establish Step B or OAuth popup compatibility. A physical-pointer repeat remains
owed.
