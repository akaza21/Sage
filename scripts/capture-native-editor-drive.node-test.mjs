import assert from "node:assert/strict";
import test from "node:test";
import { driveEditor, editorChord } from "./capture-native-editor-drive.mjs";
import { parseEditorArgs } from "./capture-native-editor-catalog.mjs";
import { editorDom } from "./capture-native-editor-test-support.mjs";

test("Ctrl+G uses bounded CDP key down and key up, not a fake prompt", async () => {
  const calls = [];
  const deadline = Date.now() + 500;
  await editorChord(
    { key: async (...args) => calls.push(args) },
    "g",
    deadline,
  );
  assert.deepEqual(
    calls.map(([event]) => event),
    [
      { type: "keyDown", key: "g", ctrl: true },
      { type: "keyUp", key: "g", ctrl: true },
    ],
  );
  assert.ok(calls.every(([, at]) => at <= deadline));
});
for (const state of [
  "editor-idle",
  "outline",
  "go-to-line",
  "search-view",
  "settings-editor",
])
  test(`${state} driver reaches its DOM contract through native controls`, async () => {
    const fixture = editorDom();
    const keys = [];
    const cdp = {
      evaluate: async (expression) => fixture.window.eval(expression),
      key: async (event) => {
        keys.push(event);
        if (event.type === "keyDown" && event.key === "g" && event.ctrl)
          fixture.openGoto();
      },
    };
    try {
      const options = parseEditorArgs([
        "--harness",
        "editor",
        "--state",
        state,
        "--width",
        "1888",
      ]);
      const { expression } = await driveEditor(
        cdp,
        options,
        Date.now() + 1000,
        { midLine: 50 },
      );
      assert.equal(
        (await cdp.evaluate(expression)).predicates.namedState,
        true,
      );
      assert.equal(keys.length, 2);
      assert.equal(
        fixture.document
          .querySelector(
            '[role="treeitem"][aria-label="Toggle folder src/features/outline"]',
          )
          .getAttribute("aria-expanded"),
        "true",
      );
    } finally {
      fixture.dom.window.close();
    }
  });
test("driver refuses absent states before any CDP operation", async () => {
  let calls = 0;
  await assert.rejects(
    () =>
      driveEditor(
        {
          evaluate: async () => {
            calls++;
          },
        },
        {
          harness: "editor",
          state: "split-editor",
          width: 1888,
          height: 1096,
          caseId: "editor-split-editor",
        },
        Date.now() + 100,
        { midLine: 50 },
      ),
    /truthful-absent-state/u,
  );
  assert.equal(calls, 0);
});
for (const width of [720, 480])
  test(`Outline at ${width} uses the native drawer without pretending the Editor remains visible`, async () => {
    const fixture = editorDom("editor-idle", width);
    try {
      const cdp = {
        evaluate: async (expression) => fixture.window.eval(expression),
        key: async (event) => {
          if (event.type === "keyDown") fixture.openGoto();
        },
      };
      const options = parseEditorArgs([
        "--harness",
        "editor",
        "--state",
        "outline",
        "--width",
        String(width),
      ]);
      const { expression } = await driveEditor(
        cdp,
        options,
        Date.now() + 1000,
        { midLine: 50 },
      );
      assert.equal(
        (await cdp.evaluate(expression)).predicates.namedState,
        true,
      );
      assert.equal(
        fixture.document.querySelector(".center-region").hasAttribute("inert"),
        true,
      );
      assert.equal(fixture.document.activeElement.className, "outline-card");
    } finally {
      fixture.dom.window.close();
    }
  });
test("driver cannot claim a missing Ctrl+G widget", async () => {
  const fixture = editorDom();
  try {
    const cdp = {
      evaluate: async (expression) => fixture.window.eval(expression),
      key: async () => {},
    };
    const options = parseEditorArgs([
      "--harness",
      "editor",
      "--state",
      "go-to-line",
      "--width",
      "1888",
    ]);
    await assert.rejects(
      () => driveEditor(cdp, options, Date.now() + 150, { midLine: 50 }),
      /state-timeout/u,
    );
  } finally {
    fixture.dom.window.close();
  }
});
