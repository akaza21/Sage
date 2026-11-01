# Feature delivery status

## GAP-C review remediation

Implemented and reviewed; Windows budget validation remains red.

- Source and Changes explicitly select their destination in the originating
  Agent pane, preserving observation behavior for newly arriving artifacts.
- Staged, unstaged, and HEAD comparisons have distinct resource identities.
  Scope is derived from the persisted tab identity after layout restoration.
- The C13 component-to-native SQLite regression uses the checked-in
  `scripts/run-gap-c-snapshot-bridge.mjs` runner. A clean checkout invokes Cargo
  with at most two build jobs and two test threads. Constrained environments
  can prebuild the library tests and set `SAGE_GAP_C_NATIVE_TEST_BINARY` to the
  executable's checkout-relative path; the browser test still runs the same
  native regression.
- Component exports and helpers have separate refresh boundaries. Browser
  geometry remains covered by rendered assertions; the toolbar CSS-source
  assertion was removed.
- Vite topology expectations cover the demand-loaded resource modules and
  assert their absence from initial and ready Agent static closures.
- Codex prerequisite negative-test inputs again use distinct package/state
  locations after public-fixture path normalization; the rejection assertions
  remain unchanged.

Review hotspots: pane selection, scope restoration and subsequent Editor
navigation, native bridge startup/cleanup, lazy resource boundaries, and the
mixed-EOL resource header screenshot.

No dependencies were added. Bundle sizes and final gate results are reported
with the change handoff.

Final validation passes formatting, lint (34 existing warnings), 4,239 frontend
tests, desktop build, web bundle budgets, policy (10 tests), Rust formatting
and Clippy, 3,383 Rust library tests, and all 7 Gap-C browser tests. Cargo
retains 30 pre-existing ignored manual/external tests. The related resource
and AP-F browser run passes all 10 tests.

Windows budget validation remains unresolved: all 840 portable tests pass;
the Windows lane reports 341 passed and 6 failed process-readiness/cleanup
tests. No skips, retries, or relaxed assertions were introduced to bypass them.

The mixed-EOL win32 snapshot intentionally reflects the compact path-chip
resource header with its visible View only state.
