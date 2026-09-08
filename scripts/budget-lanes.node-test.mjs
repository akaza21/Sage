import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import {
  CHECK_LANES_EXPECTED_ORIGIN_LINES,
  CHECK_LANES_MIGRATION,
  CHECK_LANES_ORIGIN,
  discoverBudgetTestFiles,
  extractDeclaredTestTitles,
  laneGate,
  laneOfBudgetTest,
  laneTestPattern,
  NATIVE_POSIX_BUDGET_TESTS,
  ORIGINAL_BUDGET_TESTS,
  POSIX_DARWIN_ONLY_TITLE,
  POSIX_LINUX_APPLICABLE_TITLE,
  verifyBudgetClassification,
  verifyBudgetClassificationWithLanes,
  verifyBudgetInventory,
  verifyCheckLanesMigration,
  verifyCheckLanesMigrationCoverage,
  verifyCheckLanesMigrationEntries,
  verifyLaneJustifications,
} from "./budget-lanes.mjs";

// origin: NODE-BUDGET-C02 runner record for budget-lanes.node-test.mjs.
// Portable inventory guard: proves unique ownership, no unknown owner, and
// a complete origin-to-successor mapping for every original check-lanes case.

test("budget lanes claim every discovered test exactly once", () => {
  const discovered = discoverBudgetTestFiles();
  const inventory = verifyBudgetClassification(discovered);
  assert.equal(
    inventory.discovered,
    inventory.portable + inventory.windows + inventory.posix,
  );
  for (const file of discovered) {
    assert.ok(laneOfBudgetTest(file) !== undefined, file);
  }
});

test("every original check-lanes case has a declared successor", () => {
  const migration = verifyCheckLanesMigration();
  assert.equal(migration.origins, CHECK_LANES_MIGRATION.length);
  assert.ok(migration.successors > migration.origins);
  const shared = CHECK_LANES_MIGRATION.filter(
    (origin) => origin.successors.length > 1,
  );
  assert.equal(shared.length, 1);
  assert.equal(shared[0].line, 662);
  for (const successor of shared[0].successors) {
    assert.ok(successor.title?.includes("[native-"));
    assert.ok(typeof successor.rationale === "string");
  }
});

test("lane split and runner tests are explicitly recorded", () => {
  const justifications = verifyLaneJustifications();
  assert.equal(justifications.justified, 8);
  assert.ok(
    discoverBudgetTestFiles().includes(
      "q02-shared-helpers-policy.node-test.mjs",
    ),
  );
  assert.equal(
    laneOfBudgetTest("q02-shared-helpers-policy.node-test.mjs"),
    "portable",
  );
});

test("mixed check-lanes source is retired from the discovered set", () => {
  assert.ok(!discoverBudgetTestFiles().includes(CHECK_LANES_ORIGIN));
});

test("full budget inventory verifies without an expected-total freeze", () => {
  const inventory = verifyBudgetInventory();
  assert.ok(inventory.discovered >= inventory.successors);
  assert.equal(inventory.origins, 32);
  assert.equal(inventory.successors, 33);
});

test("migration verifier parses actual test declarations, not comments", () => {
  const title =
    "Windows Cargo frame parser rejects reordered and post-proof frames";
  assert.ok(
    extractDeclaredTestTitles(`test(${JSON.stringify(title)}, () => {});`).has(
      title,
    ),
  );
  assert.equal(
    extractDeclaredTestTitles(
      `// ${title}\n// origin: ${CHECK_LANES_ORIGIN}:34 ${JSON.stringify(title)}\nconst x = 1;\n`,
    ).size,
    0,
  );
  assert.equal(
    extractDeclaredTestTitles(`test.skip(${JSON.stringify(title)}, () => {});`)
      .size,
    0,
  );
});

test("migration verifier rejects deletion and comment-only spoof", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "sage-budget-spoof-"));
  try {
    const origin = CHECK_LANES_MIGRATION.find((entry) => entry.line === 34);
    const ownedFile = "check-lanes.portable.node-test.mjs";
    const spoofMigration = [
      {
        line: 34,
        title: origin.title,
        successors: [{ file: ownedFile }],
      },
    ];
    await writeFile(
      path.join(root, ownedFile),
      `// origin: ${CHECK_LANES_ORIGIN}:34 ${JSON.stringify(origin.title)}\n// ${origin.title}\nconst x = 1;\n`,
      "utf8",
    );
    assert.throws(
      () => verifyCheckLanesMigrationEntries(root, spoofMigration),
      /missing-successor-title/,
    );
    await writeFile(path.join(root, ownedFile), "const x = 1;\n", "utf8");
    assert.throws(
      () => verifyCheckLanesMigrationEntries(root, spoofMigration),
      /missing-successor-title/,
    );
    await writeFile(
      path.join(root, ownedFile),
      `// origin: ${CHECK_LANES_ORIGIN}:34 ${JSON.stringify(origin.title)}\ntest(${JSON.stringify(origin.title)}, () => {});\n`,
      "utf8",
    );
    const ok = verifyCheckLanesMigrationEntries(root, spoofMigration);
    assert.equal(ok.origins, 1);
    assert.equal(ok.successors, 1);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("frozen original checkpoint rejects an unrecorded new member", () => {
  const discovered = discoverBudgetTestFiles();
  assert.ok(
    ORIGINAL_BUDGET_TESTS.includes(
      "validate-performance-budgets.node-test.mjs",
    ),
  );
  assert.ok(!ORIGINAL_BUDGET_TESTS.includes("budget-lanes.node-test.mjs"));
  assert.throws(
    () => verifyLaneJustifications([...discovered, "evil-new.node-test.mjs"]),
    /unrecorded-test/,
  );
});

test("budget classification fails closed on unknown member", () => {
  const discovered = discoverBudgetTestFiles();
  assert.throws(
    () => verifyBudgetClassification([...discovered, "ghost.node-test.mjs"]),
    /unclassified-member/,
  );
  assert.throws(
    () => verifyBudgetClassification(discovered.slice(1)),
    /unknown-member/,
  );
});

test("budget classification fails closed on duplicate ownership", () => {
  const discovered = discoverBudgetTestFiles();
  const duplicate = discovered[0];
  assert.throws(
    () =>
      verifyBudgetClassificationWithLanes(
        {
          portable: [duplicate],
          windows: [duplicate],
          posix: [],
        },
        [duplicate],
      ),
    /duplicate-owner/,
  );
});

test("migration coverage fails closed on omitted original origin", () => {
  const trimmed = CHECK_LANES_MIGRATION.filter((entry) => entry.line !== 34);
  assert.ok(
    CHECK_LANES_EXPECTED_ORIGIN_LINES.includes(34),
    "34 is a frozen expected origin",
  );
  assert.throws(
    () => verifyCheckLanesMigrationCoverage(trimmed),
    /omitted-origin/,
  );
});

test("posix sub-lanes separate Linux escalation from the Darwin capability gate", async () => {
  assert.equal(laneTestPattern("posix-linux"), "POSIX timeout escalates");
  assert.equal(laneTestPattern("posix-darwin"), "native-POSIX/Darwin");
  assert.equal(laneGate("posix-linux", "linux").runnable, true);
  assert.equal(laneGate("posix-linux", "win32").runnable, false);
  assert.equal(laneGate("posix-darwin", "darwin").runnable, true);
  assert.equal(laneGate("posix-darwin", "linux").runnable, false);
  assert.equal(laneGate("posix-darwin", "win32").runnable, false);
  assert.ok(!("skip" in laneGate("posix-darwin", "linux")));
  const posixFile = NATIVE_POSIX_BUDGET_TESTS[0];
  const body = await readFile(
    path.join(path.dirname(new URL(import.meta.url).pathname), posixFile),
    "utf8",
  );
  const declared = extractDeclaredTestTitles(body, posixFile);
  assert.ok(
    declared.has(POSIX_LINUX_APPLICABLE_TITLE),
    "escalation case is a real test declaration",
  );
  assert.ok(
    declared.has(POSIX_DARWIN_ONLY_TITLE),
    "Darwin arm is a real separately named test declaration",
  );
});
