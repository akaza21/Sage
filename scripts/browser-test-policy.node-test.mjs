import assert from "node:assert/strict";
import test from "node:test";
import {
  bypassErrors,
  retryGuardErrors,
  UI_RULE_MAX_TOLERANCE_PX,
  uiRuleErrors,
} from "./browser-test-policy.mjs";

const spec = "tests/ui/browser-x.visual.ts";

test("browser tests may not be skipped, narrowed, inverted, disabled or retried", () => {
  for (const line of [
    'test.skip("x", () => {});',
    'test.only("x", () => {});',
    'test.fixme("x", () => {});',
    "test.fail();",
    'it.fails("x", () => {});',
    'test.skipIf(true)("x", () => {});',
    'test.describe.skip("x", () => {});',
    'context.skip("unavailable");',
    "testInfo.skip();",
    'xit("x", () => {});',
    'xdescribe("x", () => {});',
    "test.describe.configure({ retries });",
    'const url = "http://x"; test.skip();',
  ])
    assert.equal(bypassErrors(spec, line).length, 1, line);
  for (const line of [
    "#[ignore]",
    '#[ignore = "manual"]',
    "#[cfg_attr(not(windows), ignore)]",
  ])
    assert.equal(
      bypassErrors("src-tauri/src/browser/tests.rs", line).length,
      1,
      line,
    );
  assert.deepEqual(
    bypassErrors(
      spec,
      '// test.skip is forbidden\n * test.only too\ntest("x", () => {});',
    ),
    [],
  );
  assert.deepEqual(
    bypassErrors(
      "src-tauri/src/browser/downloads.rs",
      "struct Policy { retries: u32 }",
    ),
    [],
  );
});

test("browser visual specs must install the retry guard", () => {
  assert.equal(retryGuardErrors(spec, 'test("x", () => {});').length, 1);
  assert.deepEqual(retryGuardErrors(spec, "forbidBrowserRetries();"), []);
  assert.deepEqual(retryGuardErrors("tests/ui/agent.visual.ts", ""), []);
});

test("UI rules cite a reference, a source, a known measure and a tight tolerance", () => {
  const references = ["14-codex-browser.png"];
  const rule = {
    id: "toolbar-height",
    ref: "reference/14",
    source: "CP-16",
    selector: ".toolbar",
    measure: "height",
    expected: 44,
    tolerancePx: 1,
  };
  assert.deepEqual(
    uiRuleErrors("rules.json", { rules: [rule] }, references),
    [],
  );
  assert.deepEqual(
    uiRuleErrors("rules.json", { rules: [{ ...rule, ref: "reference/59a" }] }, [
      "59a-browser-annotate-hover-link.png",
    ]),
    [],
  );
  for (const broken of [
    { ...rule, ref: "reference/99" },
    { ...rule, ref: "somewhere" },
    { ...rule, source: undefined },
    { ...rule, source: "  " },
    { ...rule, selector: "" },
    { ...rule, measure: "Height" },
    { ...rule, expected: "44" },
    { ...rule, measure: "left-of" },
    { ...rule, measure: "right-inset" },
    { ...rule, tolerancePx: UI_RULE_MAX_TOLERANCE_PX + 1 },
    { ...rule, tolerancePx: -1 },
    { ...rule, tolerancePx: Number.NaN },
  ])
    assert.equal(
      uiRuleErrors("rules.json", { rules: [broken] }, references).length,
      1,
      JSON.stringify(broken),
    );
  assert.equal(
    uiRuleErrors("rules.json", { rules: [rule, rule] }, references).length,
    1,
  );
});
