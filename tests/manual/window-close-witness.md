# Native window close witness (pending)

Run in a native Sage build when the shared native test slot is available. Record the build SHA and result for each case.

- Click the title-bar X. The workbench flushes its pending state and the window closes.
- Launch again, then press Alt+F4. The same orderly shutdown completes.
- Start an active agent Turn, then close the window. The Turn is disposed through the normal close path and the window closes.
- If a close stage fails, Sage shows the close error and stays open; a later close request retries. A second close request while the first close is pending follows the window destroy branch.

This witness is pending; unit tests cover the event port and request state, not the native operating-system close sequence.

2026-10-08 attempt: a disposable native Sage build from `554482c6` was launched,
but the Windows computer-use helper could not activate its uniquely identified
window (`GetCursorPos failed: Access is denied`, `0x80070005`). No title-bar X,
Alt+F4, active-Turn close, or failure/retry branch was dispatched. All cases
remain pending; do not infer native-close acceptance from this attempt.
