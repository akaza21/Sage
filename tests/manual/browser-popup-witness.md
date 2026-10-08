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
