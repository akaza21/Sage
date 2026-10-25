import assert from "node:assert/strict";
import test from "node:test";
import { editorDom } from "./capture-native-editor-test-support.mjs";
import {
  editorObservation,
  assertEditorState,
} from "./capture-native-editor-dom.mjs";
import { validateEditorProbe } from "./capture-native-editor-catalog.mjs";

for (const [state, remove] of [
  ["editor-idle", '[aria-label="Explorer project tools"]'],
  ["outline", ".outline-card"],
  ["go-to-line", '[role="search"] [role="status"]'],
  ["search-view", ".workspace-search__results, .workspace-search__file-group"],
  ["settings-editor", "#editor-font-size"],
])
  test(`${state} requires its own DOM witness, not just Editor mode`, () => {
    const fixture = editorDom(state);
    try {
      assert.equal(fixture.observe().predicates.namedState, true);
      assert.equal(
        fixture.window.eval(editorObservation(fixture.context)).predicates
          .namedState,
        true,
      );
      fixture.document
        .querySelectorAll(remove)
        .forEach((node) => node.remove());
      assert.throws(
        () => assertEditorState(fixture.observe()),
        /editor-state-drift/u,
      );
    } finally {
      fixture.dom.window.close();
    }
  });
test("wrong file, hidden widget, wrong caret and collapsed results fail closed", () => {
  for (const [state, mutate] of [
    [
      "editor-idle",
      (document) => {
        document.querySelector('[title="Caret position"]').textContent =
          "Ln 1, Col 1";
      },
    ],
    [
      "editor-idle",
      (document) => {
        document
          .querySelector(".file-surface")
          .setAttribute("aria-label", "File editor other.ts");
      },
    ],
    [
      "go-to-line",
      (document) => {
        document.querySelector('[role="search"]').hidden = true;
      },
    ],
    [
      "outline",
      (document) => {
        document.querySelector('[title="Caret position"]').textContent =
          "Ln 1, Col 1";
      },
    ],
    [
      "search-view",
      (document) => {
        document
          .querySelectorAll(".workspace-search__file-heading")
          .forEach((node) => node.setAttribute("aria-expanded", "false"));
      },
    ],
    [
      "settings-editor",
      (document) => {
        document.querySelector("#editor-font-size").disabled = true;
      },
    ],
  ]) {
    const fixture = editorDom(state);
    try {
      mutate(fixture.document);
      assert.equal(fixture.observe().predicates.namedState, false, state);
    } finally {
      fixture.dom.window.close();
    }
  }
});
test("compact idle is not relabelled with a hidden Editor", () => {
  const fixture = editorDom("editor-idle", 720);
  try {
    fixture.document.querySelector(".center-region").setAttribute("inert", "");
    fixture.document
      .querySelector(".center-region")
      .setAttribute("aria-hidden", "true");
    assert.equal(fixture.observe().predicates.caretMidFile, false);
  } finally {
    fixture.dom.window.close();
  }
});
test("Editor probes share the bounded chat envelope without chat identity fiction", () => {
  const fixture = editorDom();
  try {
    const observation = fixture.observe();
    const size = { width: 1888, height: 1096 };
    const probe = {
      schemaVersion: 1,
      caseId: "editor-editor-idle",
      bindingId: "native-editor",
      turnId: "none",
      protocol: "native-editor-1",
      hashes: {
        build: "a".repeat(64),
        fixture: "b".repeat(64),
        source: "c".repeat(40),
        servedAsset: "a".repeat(64),
      },
      webviewVersion: "154.0.0.1",
      dpr: 1,
      dimensions: { outer: size, client: size, css: size },
      predicates: observation.predicates,
      assistantRendered: false,
      lazyFallbackVisible: false,
      elements: observation.elements,
      controls: observation.controls,
      overflow: observation.overflow,
      timing: { elapsedMs: 1, steps: [] },
      verdict: "captured",
    };
    assert.equal(validateEditorProbe(probe), probe);
    assert.throws(
      () => validateEditorProbe({ ...probe, webviewVersion: "unknown" }),
      /probe-identity/u,
    );
    assert.throws(
      () => validateEditorProbe({ ...probe, assistantRendered: true }),
      /probe-identity/u,
    );
    assert.throws(
      () => validateEditorProbe({ ...probe, bindingId: "codex-app-server" }),
      /probe-identity/u,
    );
    assert.throws(
      () => validateEditorProbe({ ...probe, dpr: 2 }),
      /dpr-mismatch/u,
    );
    assert.throws(
      () =>
        validateEditorProbe({
          ...probe,
          predicates: { namedState: true, binding: true },
        }),
      /state-predicate/u,
    );
    assert.throws(
      () => validateEditorProbe({ ...probe, caseId: "editor-find-widget" }),
      /truthful-absent-state/u,
    );
  } finally {
    fixture.dom.window.close();
  }
});
