import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import {
  EDITOR_CATALOG,
  editorCells,
  editorCell,
  parseEditorArgs,
} from "./capture-native-editor-catalog.mjs";

test("ED-0d catalogue retains all twelve names and eighteen requested cells", () => {
  assert.deepEqual(
    EDITOR_CATALOG.map(({ state }) => state),
    [
      "editor-idle",
      "tabs-many",
      "split-editor",
      "outline",
      "find-widget",
      "find-replace",
      "multi-cursor",
      "go-to-line",
      "quick-open",
      "command-palette",
      "search-view",
      "settings-editor",
    ],
  );
  assert.equal(editorCells().length, 18);
  assert.equal(editorCells().filter(({ supported }) => supported).length, 7);
  assert.ok(editorCells().every(({ reason }) => reason.length > 20));
});
test("absent features and compact idle cannot be admitted or substituted", () => {
  for (const cell of editorCells()) {
    const args = [
      "--harness",
      "editor",
      "--state",
      cell.state,
      "--width",
      String(cell.width),
    ];
    if (cell.supported)
      assert.equal(parseEditorArgs(args).caseId, "editor-" + cell.state);
    else assert.throws(() => parseEditorArgs(args), /truthful-absent-state/u);
  }
  assert.throws(() => editorCell("go-to-line", 480), /editor-state-width/u);
  assert.throws(() => editorCell("../fake", 1888), /editor-state-width/u);
});
test("absence reasons stay grounded in shipped product boundaries", () => {
  const source = (file) =>
    readFileSync(new URL("../" + file, import.meta.url), "utf8");
  assert.match(source("AGENTS.md"), /one File \/ Diff \/ Settings tab strip/u);
  assert.match(source("src/domain/keymapRegistry.ts"), /reservedFor: "EP-08"/u);
  const find = source("src/features/files/FileFindBar.tsx");
  assert.match(find, /aria-label="Match case"/u);
  assert.doesNotMatch(find, /aria-label="(?:Regex|Replace)/iu);
  assert.doesNotMatch(
    source("src/features/files/CodeMirrorEditorRuntime.ts"),
    /allowMultipleSelections/u,
  );
  assert.doesNotMatch(source("src/workbench/TabStrip.tsx"), /tab\.preview/u);
  assert.match(
    source("src/workbench/useCompactWorkbenchLayout.ts"),
    /max-width: 760px/u,
  );
  assert.match(
    source("src/workbench/Workbench.tsx"),
    /aria-hidden=\{drawerOpen/u,
  );
  assert.equal(editorCell("command-palette", 1888).supported, false);
});
