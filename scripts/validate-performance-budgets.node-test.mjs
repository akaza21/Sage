import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { validatePerformanceBudgets } from "./validate-performance-budgets.mjs";

const workspaceRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);
const budgets = JSON.parse(
  readFileSync(
    path.join(workspaceRoot, "docs", "performance", "budgets.json"),
    "utf8",
  ),
);
const summary = JSON.parse(
  readFileSync(
    path.join(
      workspaceRoot,
      "docs",
      "work-items",
      "ADE-0004-windows-idle-footprint",
      "evidence",
      "curated-summary.json",
    ),
    "utf8",
  ),
);
const ade0005 = JSON.parse(
  readFileSync(
    path.join(
      workspaceRoot,
      "docs",
      "work-items",
      "ADE-0005-project-lifecycle",
      "evidence",
      "curated-summary.json",
    ),
    "utf8",
  ),
);

test("curated ADE-0004 evidence satisfies all machine-readable budgets", () => {
  assert.deepEqual(validatePerformanceBudgets(budgets, summary).errors, []);
});

test("active absolute P95 regressions fail even when the median passes", () => {
  const regressed = structuredClone(summary);
  const diff = regressed.states.find((state) => state.name === "diff");
  diff.privateMiB.p95 = 253.5;
  diff.overControlMiB.p95 =
    diff.privateMiB.p95 - regressed.control.privateMiB.p95;
  const { errors } = validatePerformanceBudgets(budgets, regressed);
  assert.ok(
    errors.some((error) => error.startsWith("diff absolute private P95 MiB:")),
  );
});

test("control-relative P95 regressions fail below the absolute ceiling", () => {
  const regressed = structuredClone(summary);
  const diff = regressed.states.find((state) => state.name === "diff");
  diff.privateMiB.p95 = 252.7;
  diff.overControlMiB.p95 =
    diff.privateMiB.p95 - regressed.control.privateMiB.p95;
  const { errors } = validatePerformanceBudgets(budgets, regressed);
  assert.ok(
    errors.some((error) =>
      error.startsWith("diff over-control private P95 MiB:"),
    ),
  );
  assert.ok(
    !errors.some((error) => error.startsWith("diff absolute private P95 MiB:")),
  );
});

test("active ADE-0005 evidence applies the unchanged 50 ms interaction gate", () => {
  const candidate = structuredClone(ade0005);
  candidate.workItem = "ADE-0005";
  candidate.runtime.chat.inputToPaintP95Ms = 50.202;
  const { errors } = validatePerformanceBudgets(budgets, candidate);
  assert.ok(errors.includes("chat input-to-paint P95 ms: 50.202 exceeds 50"));
});

test("active ADE-0005 evidence must identify its work item", () => {
  const candidate = structuredClone(ade0005);
  delete candidate.workItem;
  const { errors } = validatePerformanceBudgets(budgets, candidate);
  assert.ok(errors.includes("active summary must declare workItem=ADE-0005"));
});
