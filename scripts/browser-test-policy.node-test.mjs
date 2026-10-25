import assert from "node:assert/strict";
import test from "node:test";
import {
  bypassErrors,
  UI_RULE_MAX_TOLERANCE_PX,
  uiRuleErrors,
} from "./browser-test-policy.mjs";

test("browser tests may not be skipped, narrowed, expected to fail or retried", () => {
  for (const line of [
    'test.skip("x", () => {});',
    'test.only("x", () => {});',
    'test.fixme("x", () => {});',
    "test.fail();",
    'context.skip("unavailable");',
    'describe.skip("x", () => {});',
    "#[ignore]",
    '#[ignore = "manual"]',
    "test.describe.configure({ retries: 2 });",
  ])
    assert.equal(
      bypassErrors("tests/ui/browser-x.visual.ts", line).length,
      1,
      line,
    );
  assert.deepEqual(
    bypassErrors(
      "tests/ui/browser-x.visual.ts",
      '// test.skip is forbidden\ntest("x", () => {});',
    ),
    [],
  );
});

test("UI rules cite a Codex reference on disk, a measurement source and a tight tolerance", () => {
  const references = ["14-codex-browser.png"];
  const rule = {
    id: "toolbar-height",
    ref: "reference/14",
    source: "CP-16",
    tolerancePx: 1,
  };
  assert.deepEqual(
    uiRuleErrors("rules.json", { rules: [rule] }, references),
    [],
  );
  for (const broken of [
    { ...rule, ref: "reference/99" },
    { ...rule, ref: "somewhere" },
    { ...rule, source: undefined },
    { ...rule, tolerancePx: UI_RULE_MAX_TOLERANCE_PX + 1 },
  ])
    assert.equal(
      uiRuleErrors("rules.json", { rules: [broken] }, references).length,
      1,
    );
  assert.equal(
    uiRuleErrors("rules.json", { rules: [rule, rule] }, references).length,
    1,
  );
});
