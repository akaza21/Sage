import { spawnSync } from "node:child_process";
import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import ts from "typescript";

// Explicit Node-budget platform lanes. The old implicit
// `node --test scripts/*.node-test.mjs` glob mixed portable checks with
// native Windows/Job and POSIX process-group proofs; every discovered
// budget test now has exactly one owning lane below.

const testSuffix = ".node-test.mjs";

export const PORTABLE_BUDGET_TESTS = Object.freeze([
  "acp-transport-parity.node-test.mjs",
  "acp05-native-reload-capture.node-test.mjs",
  "ade0006a-c0b-paint-receipt.node-test.mjs",
  "ade0006a-package-binary.node-test.mjs",
  "ade0006a-package-build-home.node-test.mjs",
  "ade0006a-package-build-identity.node-test.mjs",
  "ade0006a-package-build.node-test.mjs",
  "ade0006a-package-environment.node-test.mjs",
  "ade0006a-package-file-set.node-test.mjs",
  "ade0006a-package-handoff.node-test.mjs",
  "ade0006a-package-members.node-test.mjs",
  "ade0006a-package-participant-validation.node-test.mjs",
  "ade0006a-package-provenance.node-test.mjs",
  "ade0006a-package-receipt-provenance.node-test.mjs",
  "ade0006a-package-retention.node-test.mjs",
  "ade0006a-package-snapshot.node-test.mjs",
  "ade0006a-package-source-stage.node-test.mjs",
  "ade0006a-package-tree-set.node-test.mjs",
  "ade0006a-package-tree.node-test.mjs",
  "acp-native-cases.node-test.mjs",
  "acp-native-pi.node-test.mjs",
  "app-smoke-server.node-test.mjs",
  "beta0001-acceptance-contract.node-test.mjs",
  "beta0001-acp05-pairing-receipt.node-test.mjs",
  "beta0001-candidate-campaign.node-test.mjs",
  "beta0001-candidate-package.node-test.mjs",
  "beta0001-candidate-verifier.node-test.mjs",
  "beta0001-macos-shared-contract-ci.node-test.mjs",
  "beta0001-source-functional-profile.node-test.mjs",
  "budget-lanes.node-test.mjs",
  "capture-native-chat.node-test.mjs",
  "capture-native-chat-pane.node-test.mjs",
  "capture-native-chat-source.node-test.mjs",
  "capture-native-chat-sizing.node-test.mjs",
  "capture-native-chat-turn.node-test.mjs",
  "check-harness-trace-privacy.node-test.mjs",
  "check-policy.node-test.mjs",
  "check-lanes.portable.node-test.mjs",
  "ci-rust-phase.node-test.mjs",
  "css-highlight-policy.node-test.mjs",
  "css-hygiene-policy.node-test.mjs",
  "generate-source-manifest.node-test.mjs",
  "gate.node-test.mjs",
  "hr6-access-mode.node-test.mjs",
  "hr6-cancel-evidence.node-test.mjs",
  "hr6-chat-scope.node-test.mjs",
  "hr6-diff-evidence.node-test.mjs",
  "hr6-harness-selection.node-test.mjs",
  "hr6-permission-evidence.node-test.mjs",
  "hr6-tool-evidence.node-test.mjs",
  "htmlCompletionSchema.node-test.mjs",
  "lane.node-test.mjs",
  "q02-shared-helpers-policy.node-test.mjs",
  "validate-ade0006a-c0b.node-test.mjs",
  "validate-performance-budgets.node-test.mjs",
  "verify-web-bundle.node-test.mjs",
  "workbench-transport-parity.node-test.mjs",
]);

export const NATIVE_WINDOWS_BUDGET_TESTS = Object.freeze([
  "acp05-owned-launcher.node-test.mjs",
  "chat-performance-budget.node-test.mjs",
  "ade0006a-c0b-campaign-launch.node-test.mjs",
  "ade0006a-c0b-collector-provenance.node-test.mjs",
  "app-smoke-cli.node-test.mjs",
  "beta0001-installed-smoke-settlement.node-test.mjs",
  "beta0001-installed-smoke.node-test.mjs",
  "beta0001-windows-locked-loader.node-test.mjs",
  "check-lanes.native-windows.node-test.mjs",
  "hr6-live-run.node-test.mjs",
  "installed-smoke-identity.node-test.mjs",
  "process-identity-cleanup.node-test.mjs",
  "source-functional-acp04-live-probe.node-test.mjs",
  "source-functional-av01-preview.node-test.mjs",
  "source-functional-av01-preview-process.node-test.mjs",
  "source-functional-native-ide-cdp.node-test.mjs",
  "source-functional-native-ide-journey.node-test.mjs",
  "windows-job.node-test.mjs",
]);

export const NATIVE_POSIX_BUDGET_TESTS = Object.freeze([
  "check-lanes.native-posix.node-test.mjs",
]);

// Frozen checkpoint membership at 2ea1bf7: 45 scripts/*.node-test.mjs
// members (32 portable whole-file owners, 12 native-Windows whole-file
// owners, 1 mixed check-lanes migration source). This list is the audit
// fixed point; current owner arrays must not self-label an added file as
// preexisting. New split/runner files are admitted only via
// LANE_JUSTIFICATIONS below.
export const ORIGINAL_BUDGET_TESTS = Object.freeze([
  "acp-transport-parity.node-test.mjs",
  "acp05-native-reload-capture.node-test.mjs",
  "acp05-owned-launcher.node-test.mjs",
  "ade0006a-c0b-campaign-launch.node-test.mjs",
  "ade0006a-c0b-collector-provenance.node-test.mjs",
  "ade0006a-c0b-paint-receipt.node-test.mjs",
  "ade0006a-package-binary.node-test.mjs",
  "ade0006a-package-build-home.node-test.mjs",
  "ade0006a-package-build-identity.node-test.mjs",
  "ade0006a-package-build.node-test.mjs",
  "ade0006a-package-environment.node-test.mjs",
  "ade0006a-package-file-set.node-test.mjs",
  "ade0006a-package-handoff.node-test.mjs",
  "ade0006a-package-members.node-test.mjs",
  "ade0006a-package-participant-validation.node-test.mjs",
  "ade0006a-package-provenance.node-test.mjs",
  "ade0006a-package-receipt-provenance.node-test.mjs",
  "ade0006a-package-retention.node-test.mjs",
  "ade0006a-package-snapshot.node-test.mjs",
  "ade0006a-package-source-stage.node-test.mjs",
  "ade0006a-package-tree-set.node-test.mjs",
  "ade0006a-package-tree.node-test.mjs",
  "beta0001-acceptance-contract.node-test.mjs",
  "beta0001-acp05-pairing-receipt.node-test.mjs",
  "beta0001-candidate-campaign.node-test.mjs",
  "beta0001-candidate-package.node-test.mjs",
  "beta0001-candidate-verifier.node-test.mjs",
  "beta0001-installed-smoke-settlement.node-test.mjs",
  "beta0001-installed-smoke.node-test.mjs",
  "beta0001-macos-shared-contract-ci.node-test.mjs",
  "beta0001-source-functional-profile.node-test.mjs",
  "beta0001-windows-locked-loader.node-test.mjs",
  "check-lanes.node-test.mjs",
  "css-highlight-policy.node-test.mjs",
  "css-hygiene-policy.node-test.mjs",
  "generate-source-manifest.node-test.mjs",
  "installed-smoke-identity.node-test.mjs",
  "process-identity-cleanup.node-test.mjs",
  "source-functional-acp04-live-probe.node-test.mjs",
  "source-functional-native-ide-cdp.node-test.mjs",
  "source-functional-native-ide-journey.node-test.mjs",
  "validate-ade0006a-c0b.node-test.mjs",
  "validate-performance-budgets.node-test.mjs",
  "windows-job.node-test.mjs",
  "workbench-transport-parity.node-test.mjs",
]);

// Frozen expected original check-lanes lines: 32 cases. The migration table
// must cover exactly these lines; an omitted origin fails closed.
export const CHECK_LANES_EXPECTED_ORIGIN_LINES = Object.freeze([
  34, 75, 115, 173, 213, 288, 314, 336, 386, 443, 488, 564, 597, 620, 644, 662,
  687, 714, 815, 885, 924, 988, 1048, 1112, 1160, 1207, 1258, 1312, 1411, 1460,
  1504, 1556,
]);

// POSIX sub-gate titles: Linux executes the Unix process-group escalation
// case; the Darwin case-insensitive arm is a separately named Darwin
// capability gate, never a Linux false green and never a skip.
export const POSIX_LINUX_APPLICABLE_TITLE =
  "POSIX timeout escalates the process group after its parent exits on TERM";
export const POSIX_DARWIN_ONLY_TITLE =
  "case variants contend across target materialization on case-insensitive hosts [native-POSIX/Darwin]";
export const POSIX_LINUX_TEST_NAME_PATTERN = "POSIX timeout escalates";
export const POSIX_DARWIN_TEST_NAME_PATTERN = "native-POSIX/Darwin";

export const CHECK_LANES_ORIGIN = "check-lanes.node-test.mjs";

// Every original check-lanes case and its explicit successor(s). A shared
// origin appears once with one entry per successor; each successor declares
// the shared origin line and its platform rationale.
export const CHECK_LANES_MIGRATION = Object.freeze([
  {
    line: 34,
    title: "Windows Cargo frame parser rejects reordered and post-proof frames",
    successors: [{ file: "check-lanes.portable.node-test.mjs" }],
  },
  {
    line: 75,
    title: "developer lane requires explicit bounded targets",
    successors: [{ file: "check-lanes.portable.node-test.mjs" }],
  },
  {
    line: 115,
    title:
      "developer plan keeps Cargo filters sequential and overlaps only language lanes",
    successors: [{ file: "check-lanes.portable.node-test.mjs" }],
  },
  {
    line: 173,
    title:
      "Rust validation plans build the frontend bundle before concurrent groups",
    successors: [{ file: "check-lanes.portable.node-test.mjs" }],
  },
  {
    line: 213,
    title:
      "integration plan preserves every frontend phase and serializes Rust tools",
    successors: [{ file: "check-lanes.portable.node-test.mjs" }],
  },
  {
    line: 288,
    title: "resolved npm invocation executes without a shell command string",
    successors: [{ file: "check-lanes.portable.node-test.mjs" }],
  },
  {
    line: 314,
    title: "Windows Cargo Job preserves Cargo proxy command semantics",
    successors: [{ file: "check-lanes.native-windows.node-test.mjs" }],
  },
  {
    line: 336,
    title:
      "release lane fails closed and preserves conservative validation order",
    successors: [{ file: "check-lanes.portable.node-test.mjs" }],
  },
  {
    line: 386,
    title:
      "release package mode hands exact validated arguments to the existing campaign",
    successors: [{ file: "check-lanes.portable.node-test.mjs" }],
  },
  {
    line: 443,
    title:
      "Rust environment uses explicit paths and only enables detected sccache",
    successors: [{ file: "check-lanes.portable.node-test.mjs" }],
  },
  {
    line: 488,
    title:
      "Cargo target lease excludes a second Node process and releases cleanly",
    successors: [{ file: "check-lanes.portable.node-test.mjs" }],
  },
  {
    line: 564,
    title: "physical Cargo target aliases contend on one lease",
    successors: [{ file: "check-lanes.portable.node-test.mjs" }],
  },
  {
    line: 597,
    title: "Cargo lease namespace survives nested target materialization",
    successors: [{ file: "check-lanes.portable.node-test.mjs" }],
  },
  {
    line: 620,
    title: "nonexistent Cargo target canonicalizes its physical ancestor",
    successors: [{ file: "check-lanes.portable.node-test.mjs" }],
  },
  {
    line: 644,
    title:
      "Darwin lease identity folds case for default case-insensitive targets",
    successors: [{ file: "check-lanes.portable.node-test.mjs" }],
  },
  {
    line: 662,
    title:
      "case variants contend across target materialization on case-insensitive hosts",
    successors: [
      {
        file: "check-lanes.native-windows.node-test.mjs",
        title:
          "case variants contend across target materialization on case-insensitive hosts [native-Windows/NTFS]",
        rationale:
          "Windows arm: exercises win32 lease-identity folding on the native NTFS host capability.",
      },
      {
        file: "check-lanes.native-posix.node-test.mjs",
        title:
          "case variants contend across target materialization on case-insensitive hosts [native-POSIX/Darwin]",
        rationale:
          "Darwin arm: retains the Darwin case-insensitive contention proof by name with injected darwin identity, without a Linux-only gate.",
      },
    ],
  },
  {
    line: 687,
    title: "Cargo lease acquisition obeys its deadline without a partial owner",
    successors: [{ file: "check-lanes.portable.node-test.mjs" }],
  },
  {
    line: 714,
    title:
      "unconfirmed Cargo command settlement quarantines the lease and denies a new process",
    successors: [{ file: "check-lanes.portable.node-test.mjs" }],
  },
  {
    line: 815,
    title:
      "Windows Cargo nonzero waits for descendant zero and releases its lease",
    successors: [{ file: "check-lanes.native-windows.node-test.mjs" }],
  },
  {
    line: 885,
    title: "unavailable Windows Job proof quarantines Cargo",
    successors: [{ file: "check-lanes.portable.node-test.mjs" }],
  },
  {
    line: 924,
    title:
      "Windows Cargo success waits for an owned descendant tree to reach zero",
    successors: [{ file: "check-lanes.native-windows.node-test.mjs" }],
  },
  {
    line: 988,
    title:
      "Windows Cargo Job preserves argv cwd environment and bounded output",
    successors: [{ file: "check-lanes.native-windows.node-test.mjs" }],
  },
  {
    line: 1048,
    title:
      "Windows Cargo Job streams before completion and honors sink backpressure",
    successors: [{ file: "check-lanes.native-windows.node-test.mjs" }],
  },
  {
    line: 1112,
    title: "Windows Cargo Job streams both outputs near their independent caps",
    successors: [{ file: "check-lanes.native-windows.node-test.mjs" }],
  },
  {
    line: 1160,
    title:
      "Windows Cargo Job emits a truncation marker and observes grace before force",
    successors: [{ file: "check-lanes.native-windows.node-test.mjs" }],
  },
  {
    line: 1207,
    title: "Windows Cargo Job timeout proves descendant zero",
    successors: [{ file: "check-lanes.native-windows.node-test.mjs" }],
  },
  {
    line: 1258,
    title: "Windows Cargo Job abort and output overflow prove tree zero",
    successors: [{ file: "check-lanes.native-windows.node-test.mjs" }],
  },
  {
    line: 1312,
    title: "bounded runner caps concurrency and aborts sibling work on failure",
    successors: [{ file: "check-lanes.portable.node-test.mjs" }],
  },
  {
    line: 1411,
    title:
      "real command timeout terminates its spawned process tree without an orphan",
    successors: [{ file: "check-lanes.portable.node-test.mjs" }],
  },
  {
    line: 1460,
    title: "leader exit cannot falsely prove descendant settlement",
    successors: [{ file: "check-lanes.portable.node-test.mjs" }],
  },
  {
    line: 1504,
    title:
      "POSIX timeout escalates the process group after its parent exits on TERM",
    successors: [{ file: "check-lanes.native-posix.node-test.mjs" }],
  },
  {
    line: 1556,
    title:
      "newline-free output is bounded and terminates the offending command",
    successors: [{ file: "check-lanes.portable.node-test.mjs" }],
  },
]);

// Origin/justification record for every budget test introduced by the lane
// split itself (not a migration successor): the discovered file count may
// grow only through entries recorded here.
export const LANE_JUSTIFICATIONS = Object.freeze([
  {
    file: "chat-performance-budget.node-test.mjs",
    reason:
      "Native-Windows Chromium fixture checks measured CP-24 Chat mount, streaming, DOM, and heap ceilings through the production presentation seam.",
  },
  {
    file: "budget-lanes.node-test.mjs",
    reason:
      "Inventory/migration assertion for the explicit lane manifest; portable pure discovery logic.",
  },
  {
    file: "acp-native-cases.node-test.mjs",
    reason:
      "Portable checks of the bounded native fixture case catalogue and its privacy limits; no native capability required.",
  },
  {
    file: "acp-native-pi.node-test.mjs",
    reason:
      "Portable Pi replay catalogue, wire-shape, capture and privacy contract checks; no subprocess launch.",
  },
  {
    file: "capture-native-chat.node-test.mjs",
    reason:
      "Portable checks of native capture argument, probe-schema and output-path rules; the capture itself runs only by hand.",
  },
  {
    file: "capture-native-chat-source.node-test.mjs",
    reason:
      "Portable mocked source-provenance, owned-Vite launch-contract and local staging/promotion privacy checks; never launches Vite or a native app.",
  },
  {
    file: "capture-native-chat-pane.node-test.mjs",
    reason:
      "Portable single/two-pane DOM capture ownership checks using the existing jsdom dev dependency; no native capability or subprocess.",
  },
  {
    file: "capture-native-chat-sizing.node-test.mjs",
    reason:
      "Portable DPR-1 requested-client geometry and PNG dimension contracts; no native app or window is launched.",
  },
  {
    file: "capture-native-chat-turn.node-test.mjs",
    reason:
      "Portable simulated DOM readiness, bounded probe timestamp and acceptance-matrix admission checks; blocked-run coverage exits before launching an app.",
  },
  {
    file: "check-harness-trace-privacy.node-test.mjs",
    reason:
      "Portable harness-trace privacy guard regression tests inspect JSON values without native capabilities.",
  },
  {
    file: "check-policy.node-test.mjs",
    reason:
      "Portable exact-main-WebView ACL permission regression for Sage app commands; no native capability required.",
  },
  {
    file: "app-smoke-server.node-test.mjs",
    reason:
      "Portable dev-server lifecycle proofs for the smoke harness; spawn, TCP probes, and process liveness only, with no native capability.",
  },
  {
    file: "app-smoke-cli.node-test.mjs",
    reason:
      "Browser-dependent smoke CLI regression lane; drives the real app-smoke entrypoint with bundled Playwright Chromium and asserts exit status, dev-server PID death, and port release.",
  },
  {
    file: "check-lanes.portable.node-test.mjs",
    reason:
      "Portable successor lane for the 21 plan/parser/stream/dependency-injected check-lanes cases.",
  },
  {
    file: "check-lanes.native-windows.node-test.mjs",
    reason:
      "Native-Windows successor lane for the 9 Cargo/Job cases plus the Windows arm of the contention proof.",
  },
  {
    file: "check-lanes.native-posix.node-test.mjs",
    reason:
      "Native-POSIX successor lane for the Unix process-group escalation proof plus the Darwin arm of the contention proof.",
  },
  {
    file: "ci-rust-phase.node-test.mjs",
    reason:
      "Portable Rust-matrix workflow source contract; reads the committed YAML only and requires no native capability.",
  },
  {
    file: "source-functional-av01-preview.node-test.mjs",
    reason:
      "Native-Windows source-functional AV01 Preview fixture, CDP collector, and final-receipt contract tests require the Windows journey envelope.",
  },
  {
    file: "source-functional-av01-preview-process.node-test.mjs",
    reason:
      "Native-Windows owned-child process identity capture, live rejection, and disappearance proof for the AV01 Preview helper.",
  },
  {
    file: "htmlCompletionSchema.node-test.mjs",
    reason:
      "Portable BX-2 build-transform guard proof; runs the HTML schema plugin against in-memory module sources.",
  },
  {
    file: "verify-web-bundle.node-test.mjs",
    reason:
      "Portable Q04 emitted-web artifact verifier tests exact gzip limits, manifest closure, and freshness receipt rejection.",
  },
  {
    file: "q02-shared-helpers-policy.node-test.mjs",
    reason:
      "Portable Q02 bounded source-policy scanner proof; uses injected temporary filesystem entries and no platform lane capability.",
  },
  {
    file: "lane.node-test.mjs",
    reason:
      "Portable lane cleanup receipt and recovery proof; uses injected operations, temporary files, and disposable Git repositories without native lock-holder behavior.",
  },
  {
    file: "gate.node-test.mjs",
    reason:
      "Portable gate environment contract proves the deterministic Vitest worker defaults and explicit operator overrides.",
  },
  {
    file: "hr6-chat-scope.node-test.mjs",
    reason:
      "Portable HR-6 focused-Chat selector regression uses an in-memory DOM and source contract without native process control.",
  },
  {
    file: "hr6-live-run.node-test.mjs",
    reason:
      "Native-Windows HR-6 live-run proof launches PowerShell receipt, stream, and cleanup fixtures.",
  },
  {
    file: "hr6-tool-evidence.node-test.mjs",
    reason:
      "Portable HR-6 tool-evidence regression uses in-memory projections and source contracts without native process control.",
  },
  {
    file: "hr6-cancel-evidence.node-test.mjs",
    reason:
      "Portable HR-6 running-tool cancellation regression uses an in-memory DOM, bounded projections, and source contracts without native process control.",
  },
  {
    file: "hr6-access-mode.node-test.mjs",
    reason:
      "Portable HR-6 session access-mode picker regression uses an in-memory DOM and source contracts without native process control.",
  },
  {
    file: "hr6-diff-evidence.node-test.mjs",
    reason:
      "Portable HR-6 diff and file-chip evidence regression uses an in-memory DOM and bounded projections without native process control.",
  },
  {
    file: "hr6-permission-evidence.node-test.mjs",
    reason:
      "Portable HR-6 permission-card evidence regression uses an in-memory DOM, bounded projections, and source contracts without native process control.",
  },
  {
    file: "hr6-harness-selection.node-test.mjs",
    reason:
      "Portable HR-6 harness selection outcome and opt-in flag regression evaluates pure classification functions without native process control.",
  },
]);

// Frozen preexisting set is ORIGINAL_BUDGET_TESTS above, not a filter of the
// current owner arrays. Deriving "preexisting" from the current lists would
// let an added file self-label as preexisting; only ORIGINAL plus an explicit
// LANE_JUSTIFICATIONS entry admits a new member.

export function scriptsDirectory() {
  return path.dirname(fileURLToPath(import.meta.url));
}

export function repositoryRoot() {
  return path.dirname(scriptsDirectory());
}

export function discoverBudgetTestFiles(directory = scriptsDirectory()) {
  return readdirSync(directory, { withFileTypes: true })
    .filter((entry) => entry.isFile() && entry.name.endsWith(testSuffix))
    .map((entry) => entry.name)
    .sort();
}

export function laneOfBudgetTest(file) {
  if (PORTABLE_BUDGET_TESTS.includes(file)) return "portable";
  if (NATIVE_WINDOWS_BUDGET_TESTS.includes(file)) return "windows";
  if (NATIVE_POSIX_BUDGET_TESTS.includes(file)) return "posix";
  return undefined;
}

function fail(code, detail) {
  const error = new Error(`budget-lanes:${code} ${detail}`);
  error.code = `budget-lanes/${code}`;
  throw error;
}

// Parse actual Node test declarations via the existing TypeScript dependency.
// Returns the set of first-argument titles passed to direct `test(...)`
// calls. Titles appearing only in comments or strings elsewhere do not count.
export function extractDeclaredTestTitles(
  sourceText,
  fileName = "candidate.mjs",
) {
  const source = ts.createSourceFile(
    fileName,
    sourceText,
    ts.ScriptTarget.Latest,
    true,
  );
  const titles = new Set();
  function visit(node) {
    if (
      ts.isCallExpression(node) &&
      ts.isIdentifier(node.expression) &&
      node.expression.text === "test" &&
      node.arguments.length >= 1
    ) {
      const first = node.arguments[0];
      if (
        ts.isStringLiteral(first) ||
        ts.isNoSubstitutionTemplateLiteral(first)
      ) {
        titles.add(first.text);
      }
    }
    ts.forEachChild(node, visit);
  }
  visit(source);
  return titles;
}

// Complete unique classification: every discovered member has exactly one
// owner, and every listed member is discovered. Accepts injected lane lists
// so negative regressions can prove fail-closed behavior without mutating
// the frozen manifest.
export function verifyBudgetClassificationWithLanes(
  lanes,
  discovered = discoverBudgetTestFiles(),
) {
  const portable = lanes.portable ?? [];
  const windows = lanes.windows ?? [];
  const posix = lanes.posix ?? [];
  const seen = new Set();
  for (const file of [...portable, ...windows, ...posix]) {
    if (seen.has(file)) fail("duplicate-owner", file);
    seen.add(file);
  }
  for (const file of discovered) {
    if (!seen.has(file)) fail("unclassified-member", file);
  }
  for (const file of seen) {
    if (!discovered.includes(file)) fail("unknown-member", file);
  }
  return {
    discovered: discovered.length,
    portable: portable.length,
    windows: windows.length,
    posix: posix.length,
  };
}

export function verifyBudgetClassification(
  discovered = discoverBudgetTestFiles(),
) {
  return verifyBudgetClassificationWithLanes(
    {
      portable: PORTABLE_BUDGET_TESTS,
      windows: NATIVE_WINDOWS_BUDGET_TESTS,
      posix: NATIVE_POSIX_BUDGET_TESTS,
    },
    discovered,
  );
}

// Coverage half: the migration table must cover exactly the frozen expected
// origin lines, with the sole justified one-to-many split at line 662.
export function verifyCheckLanesMigrationCoverage(
  migration = CHECK_LANES_MIGRATION,
) {
  const expected = new Set(CHECK_LANES_EXPECTED_ORIGIN_LINES);
  const actual = new Set(migration.map((origin) => origin.line));
  for (const line of expected) {
    if (!actual.has(line)) {
      fail("omitted-origin", `${CHECK_LANES_ORIGIN}:${line}`);
    }
  }
  for (const line of actual) {
    if (!expected.has(line))
      fail("unknown-origin", `${CHECK_LANES_ORIGIN}:${line}`);
  }
  const shared = migration.filter((origin) => origin.successors.length > 1);
  if (shared.length !== 1 || shared[0].line !== 662) {
    fail(
      "unexpected-split",
      `only ${CHECK_LANES_ORIGIN}:662 may declare multiple successors`,
    );
  }
  for (const origin of migration) {
    if (origin.successors.length === 0) {
      fail("orphaned-origin", `${CHECK_LANES_ORIGIN}:${origin.line}`);
    }
  }
  return { origins: migration.length };
}

// Entries half: every listed successor must declare an actual `test(...)`
// title (parsed via TypeScript, not `body.includes`) and cite its origin
// line. A comment-only spoof or deleted test fails here.
export function verifyCheckLanesMigrationEntries(
  directory = scriptsDirectory(),
  migration = CHECK_LANES_MIGRATION,
) {
  let successors = 0;
  for (const origin of migration) {
    if (origin.successors.length === 0) {
      fail("orphaned-origin", `${CHECK_LANES_ORIGIN}:${origin.line}`);
    }
    for (const successor of origin.successors) {
      successors += 1;
      if (laneOfBudgetTest(successor.file) === undefined) {
        fail("unknown-successor", successor.file);
      }
      const body = readFileSync(path.join(directory, successor.file), "utf8");
      const expectedTitle = successor.title ?? origin.title;
      const declared = extractDeclaredTestTitles(body, successor.file);
      if (!declared.has(expectedTitle)) {
        fail("missing-successor-title", `${successor.file} :: ${origin.title}`);
      }
      if (!body.includes(`${CHECK_LANES_ORIGIN}:${origin.line}`)) {
        fail("missing-origin-citation", `${successor.file}:${origin.line}`);
      }
      if (successor.title !== undefined && successor.rationale === undefined) {
        fail("missing-split-rationale", successor.file);
      }
    }
  }
  return { origins: migration.length, successors };
}

// Complete origin-to-successor mapping: coverage plus parsed declarations.
export function verifyCheckLanesMigration(
  directory = scriptsDirectory(),
  migration = CHECK_LANES_MIGRATION,
) {
  const coverage = verifyCheckLanesMigrationCoverage(migration);
  const entries = verifyCheckLanesMigrationEntries(directory, migration);
  return { origins: coverage.origins, successors: entries.successors };
}

// New split/runner files are admitted only through an explicit record
// against the frozen ORIGINAL checkpoint, never by self-labeling in the
// current owner arrays. The retired mixed source is expected absent from
// discovery; every other original member must still be discovered.
export function verifyLaneJustifications(
  discovered = discoverBudgetTestFiles(),
  options = {},
) {
  const original = new Set(options.original ?? ORIGINAL_BUDGET_TESTS);
  const justifications = options.justifications ?? LANE_JUSTIFICATIONS;
  const retired = options.retired ?? CHECK_LANES_ORIGIN;
  const justified = new Set(justifications.map((entry) => entry.file));
  for (const file of discovered) {
    if (!original.has(file) && !justified.has(file)) {
      fail("unrecorded-test", file);
    }
  }
  for (const file of justified) {
    if (!discovered.includes(file)) fail("unknown-member", file);
    if (original.has(file)) fail("justified-preexisting", file);
  }
  for (const file of original) {
    if (file === retired) continue;
    if (!discovered.includes(file)) fail("missing-original", file);
  }
  return { justified: justifications.length };
}

export function verifyBudgetInventory(directory = scriptsDirectory()) {
  const discovered = discoverBudgetTestFiles(directory);
  const classification = verifyBudgetClassification(discovered);
  const migration = verifyCheckLanesMigration(directory);
  const justifications = verifyLaneJustifications(discovered);
  return { ...classification, ...migration, ...justifiedCount(justifications) };
}

function justifiedCount(justifications) {
  return { justified: justifications.justified };
}

export function laneTestFiles(lane) {
  if (lane === "portable") return [...PORTABLE_BUDGET_TESTS];
  if (lane === "windows") return [...NATIVE_WINDOWS_BUDGET_TESTS];
  if (lane === "posix") return [...NATIVE_POSIX_BUDGET_TESTS];
  if (lane === "posix-linux") return [...NATIVE_POSIX_BUDGET_TESTS];
  if (lane === "posix-darwin") return [...NATIVE_POSIX_BUDGET_TESTS];
  fail("unknown-lane", lane);
}

export function laneTestPattern(lane) {
  if (lane === "posix-linux") return POSIX_LINUX_TEST_NAME_PATTERN;
  if (lane === "posix-darwin") return POSIX_DARWIN_TEST_NAME_PATTERN;
  return undefined;
}

export function laneGate(lane, platform = process.platform) {
  if (lane === "portable") return { runnable: true };
  if (lane === "windows") {
    return platform === "win32"
      ? { runnable: true }
      : {
          runnable: false,
          reason: `native-windows lane requires process.platform "win32" (current: ${platform})`,
        };
  }
  if (lane === "posix") {
    return platform === "win32"
      ? {
          runnable: false,
          reason: `native-posix lane requires a Unix host (current: ${platform})`,
        }
      : { runnable: true };
  }
  if (lane === "posix-linux") {
    return platform === "win32"
      ? {
          runnable: false,
          reason: `native-posix-linux lane requires a Unix host (current: ${platform})`,
        }
      : { runnable: true };
  }
  if (lane === "posix-darwin") {
    return platform === "darwin"
      ? { runnable: true }
      : {
          runnable: false,
          reason: `native-posix-darwin lane requires process.platform "darwin" with a case-insensitive host capability (current: ${platform})`,
        };
  }
  return fail("unknown-lane", lane);
}

export function environmentForBudgetLane(environment = process.env) {
  if (process.platform !== "win32") return environment;
  const systemRoot = environment.SystemRoot ?? environment.SYSTEMROOT;
  if (!systemRoot || !path.isAbsolute(systemRoot)) return environment;
  const inboxModules = path.join(
    systemRoot,
    "System32",
    "WindowsPowerShell",
    "v1.0",
    "Modules",
  );
  const modulePaths = (environment.PSModulePath ?? "")
    .split(path.delimiter)
    .filter((candidate) => candidate.length > 0);
  const retained = modulePaths.filter(
    (candidate) => candidate.toLowerCase() !== inboxModules.toLowerCase(),
  );
  return {
    ...environment,
    PSModulePath: [inboxModules, ...retained].join(path.delimiter),
  };
}

function runNodeTests(files, directory, { testNamePattern, environment } = {}) {
  // Lane tests assume the repository root as cwd (repo-relative evidence,
  // manifest, and Cargo paths); the file arguments stay absolute.
  const root = path.dirname(directory);
  const args = ["--test", "--test-concurrency=1"];
  if (testNamePattern !== undefined) {
    args.push(`--test-name-pattern=${testNamePattern}`);
  }
  args.push(...files.map((file) => path.join(directory, file)));
  const result = spawnSync(process.execPath, args, {
    cwd: root,
    env: environment,
    stdio: "inherit",
  });
  if (result.error) throw result.error;
  return result.status ?? 1;
}

export function runBudgetLane(lane, { directory = scriptsDirectory() } = {}) {
  const inventory = verifyBudgetInventory(directory);
  const gate = laneGate(lane);
  if (!gate.runnable) {
    process.stderr.write(
      `BUDGET LANE UNVERIFIED (${lane}): ${gate.reason}; not counted as passed.\n`,
    );
    return { status: 2, inventory, lane, verified: false };
  }
  const files = laneTestFiles(lane);
  const pattern = laneTestPattern(lane);
  const patternNote = pattern !== undefined ? ` pattern ${pattern}` : "";
  process.stdout.write(
    `budget lane ${lane}: inventory ${inventory.discovered} discovered (${inventory.portable} portable/${inventory.windows} windows/${inventory.posix} posix), ${inventory.origins} origins to ${inventory.successors} successors, ${inventory.justified} lane records${patternNote}.\n`,
  );
  const status = runNodeTests(files, directory, {
    testNamePattern: pattern,
    environment: lane === "windows" ? environmentForBudgetLane() : undefined,
  });
  return { status, inventory, lane, verified: status === 0 };
}

function printUsage() {
  process.stderr.write(
    "usage: budget-lanes.mjs --lane portable|windows|posix|posix-linux|posix-darwin|--inventory|--check\n",
  );
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const argument = process.argv[2];
  const directory = scriptsDirectory();
  if (argument === "--inventory") {
    const inventory = verifyBudgetInventory(directory);
    process.stdout.write(`budget inventory ok: ${JSON.stringify(inventory)}\n`);
  } else if (argument === "--lane") {
    const lane = process.argv[3];
    if (
      lane !== "portable" &&
      lane !== "windows" &&
      lane !== "posix" &&
      lane !== "posix-linux" &&
      lane !== "posix-darwin"
    ) {
      printUsage();
      process.exit(2);
    }
    const { status } = runBudgetLane(lane, { directory });
    process.exit(status);
  } else if (argument === "--check") {
    const inventory = verifyBudgetInventory(directory);
    const portable = runBudgetLane("portable", { directory });
    if (portable.status !== 0) process.exit(portable.status);
    // Linux executes the retained Unix process-group escalation case via the
    // posix-linux sub-lane. The Darwin case-insensitive arm stays a separate
    // Darwin capability gate, never a Linux false green and never a skip.
    // Windows stays a native Windows gate.
    if (process.platform !== "win32") {
      const posixLinux = runBudgetLane("posix-linux", { directory });
      if (posixLinux.status !== 0) process.exit(posixLinux.status);
    }
    const outstanding = [];
    for (const lane of ["windows", "posix-darwin"]) {
      const nativeOnThisHost =
        (lane === "windows" && process.platform === "win32") ||
        (lane === "posix-darwin" && process.platform === "darwin");
      if (nativeOnThisHost) {
        const result = runBudgetLane(lane, { directory });
        if (result.status !== 0) process.exit(result.status);
      } else {
        outstanding.push(
          `npm run test:budgets:${lane === "posix-darwin" ? "posix:darwin" : lane}`,
        );
      }
    }
    // The full posix file (both tests) remains explicitly runnable via
    // `npm run test:budgets:posix`; on Linux it fails closed on the Darwin
    // arm instead of passing vacuously. The composite above runs only the
    // Linux-applicable escalation sub-lane and names the Darwin gate
    // outstanding.
    if (process.platform === "win32") {
      outstanding.push("npm run test:budgets:posix:linux");
    }
    if (outstanding.length > 0) {
      process.stdout.write(
        `budget check: portable passed (${inventory.portable}/${inventory.discovered}); posix-linux escalation passed where applicable; native outstanding, not passed: ${outstanding.join(", ")}.\n`,
      );
    } else {
      process.stdout.write("budget check: all applicable lanes passed.\n");
    }
  } else {
    printUsage();
    process.exit(2);
  }
}
