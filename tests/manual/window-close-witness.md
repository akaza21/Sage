# Native window close witness (pending)

Run in a native Sage build when the shared native test slot is available. Record the build SHA and result for each case.

- Click the title-bar X. The workbench flushes its pending state and the window closes.
- Launch again, then press Alt+F4. The same orderly shutdown completes.
- Start an active agent Turn, then close the window. The Turn is disposed through the normal close path and the window closes.
- If a close stage fails, Sage shows the close error and stays open; a later close request retries. A second close request while the first close is pending follows the window destroy branch.

This witness is pending; unit tests cover the event port and request state, not the native operating-system close sequence.
