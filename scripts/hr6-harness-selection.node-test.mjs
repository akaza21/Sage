import assert from "node:assert/strict";
import test from "node:test";

function load() {
  return import("./hr6-harness-selection.mjs");
}

test("harness selection states are a frozen closed vocabulary", async () => {
  const { HARNESS_SELECTION_STATES } = await load();
  assert.deepEqual(HARNESS_SELECTION_STATES, [
    "selected",
    "absent",
    "disabled",
    "readiness-timeout",
    "alternative",
  ]);
  assert.ok(Object.isFrozen(HARNESS_SELECTION_STATES));
});

test("an absent or disabled picker row is not drivable", async () => {
  const { harnessSelectionOutcome } = await load();
  assert.deepEqual(harnessSelectionOutcome("absent"), {
    kind: "harness-not-drivable",
    state: "absent",
  });
  assert.deepEqual(harnessSelectionOutcome("disabled"), {
    kind: "harness-not-drivable",
    state: "disabled",
  });
});

test("a selected row and an unready harness stay distinct outcomes", async () => {
  const { harnessSelectionOutcome } = await load();
  assert.deepEqual(harnessSelectionOutcome("selected"), {
    kind: "selected",
    state: "selected",
  });
  assert.deepEqual(harnessSelectionOutcome("readiness-timeout"), {
    kind: "harness-not-ready",
    state: "readiness-timeout",
  });
  assert.deepEqual(harnessSelectionOutcome("alternative"), {
    kind: "harness-not-ready",
    state: "alternative",
  });
});

test("an unrecorded selection state fails closed", async () => {
  const { harnessSelectionOutcome } = await load();
  for (const state of [
    undefined,
    null,
    "",
    "Selected",
    "not-drivable",
    "harness-not-drivable",
    0,
    true,
    ["absent"],
    { state: "absent" },
  ]) {
    assert.throws(
      () => harnessSelectionOutcome(state),
      /hr6-harness-selection-invalid/u,
    );
  }
});

test("the OpenCode step is opt-in and defaults to skipped", async () => {
  const { includeOpenCodeRequested } = await load();
  assert.equal(includeOpenCodeRequested({}), false);
  assert.equal(includeOpenCodeRequested({ "--port": "9331" }), false);
  assert.equal(
    includeOpenCodeRequested({ "--include-opencode": "false" }),
    false,
  );
  assert.equal(
    includeOpenCodeRequested({ "--include-opencode": "true" }),
    true,
  );
});

test("an ambiguous OpenCode flag value fails closed", async () => {
  const { includeOpenCodeRequested } = await load();
  for (const raw of ["", "True", "1", "0", "yes", " true", "true "]) {
    assert.throws(
      () => includeOpenCodeRequested({ "--include-opencode": raw }),
      /hr6-arguments-invalid/u,
    );
  }
  for (const values of [null, undefined, "--include-opencode true", 0]) {
    assert.throws(
      () => includeOpenCodeRequested(values),
      /hr6-arguments-invalid/u,
    );
  }
});
