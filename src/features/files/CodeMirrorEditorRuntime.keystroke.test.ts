import { Text } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";

import { settleLazyModules } from "../../test/settleLazyModules";
import { createCodeMirrorEditorRuntime } from "./CodeMirrorEditorRuntime";

const EDITING = {
  matchBrackets: true,
  autoClosingBrackets: true,
  folding: true,
  occurrencesHighlight: true,
  multiCursorModifier: "alt" as const,
};

// The first editor loads the editing features and its language lazily; later
// editors reuse them. Open one first, so a cold load, which waits on the test
// runner under load, is not timed with the keystrokes.
beforeAll(async () => {
  const parent = document.createElement("div");
  document.body.append(parent);
  const runtime = createCodeMirrorEditorRuntime({
    parent,
    ariaLabel: "Edit warm.ts",
    path: "warm.ts",
    content: "call(item);\n",
    disabled: false,
    maxContentLength: 1024,
    onChange: () => undefined,
    editing: EDITING,
  });
  await settleLazyModules();
  runtime.destroy();
  parent.remove();
});

afterEach(() => {
  vi.restoreAllMocks();
  document.body.replaceChildren();
});

// The spies fail on the first keystroke that builds the whole document, so a
// few keystrokes prove it. More only add jsdom work under the test timeout.
const KEYSTROKES = 50;

function mount(content: string | Text, maxContentLength: number) {
  const parent = document.createElement("div");
  document.body.append(parent);
  const onChange = vi.fn();
  const runtime = createCodeMirrorEditorRuntime({
    parent,
    ariaLabel: "Edit big.txt",
    path: "big.txt",
    content,
    disabled: false,
    maxContentLength,
    onChange,
  });
  const element = parent.querySelector<HTMLElement>(".cm-content");
  const view = element ? EditorView.findFromDOM(element) : null;
  if (!view) throw new Error("Expected a mounted CodeMirror editor");
  return { runtime, view, onChange };
}

describe("CodeMirrorEditorRuntime keystroke cost", () => {
  it("checks the byte limit of a large document without stringifying it", () => {
    // Over a third of the limit, the old check encoded the whole document.
    const limit = 1024 * 1024;
    const { runtime, view, onChange } = mount("é".repeat(400_000), limit);
    const toString = vi.spyOn(Text.prototype, "toString");
    for (let index = 0; index < KEYSTROKES; index += 1) {
      view.dispatch({ changes: { from: index, insert: "x" } });
      // The store echoes the document back, as FileSurface does.
      runtime.update({
        ariaLabel: "Edit big.txt",
        path: "big.txt",
        content: onChange.mock.lastCall?.[0],
        documentIdentity: `big\0${index + 1}`,
        disabled: false,
        maxContentLength: limit,
        onChange,
      });
    }
    // No keystroke builds the whole document as a string.
    expect(toString).not.toHaveBeenCalled();
    expect(onChange).toHaveBeenCalledTimes(KEYSTROKES);
    expect(onChange.mock.lastCall?.[0]).toBe(view.state.doc);
    runtime.destroy();
  });

  it("keeps keystrokes off the whole document with the editing features on", async () => {
    const limit = 1024 * 1024;
    const parent = document.createElement("div");
    document.body.append(parent);
    const onChange = vi.fn();
    const props = {
      ariaLabel: "Edit big.ts",
      path: "big.ts",
      content: "call(item, item);\n".repeat(20_000),
      disabled: false,
      maxContentLength: limit,
      onChange,
      editing: EDITING,
    };
    const runtime = createCodeMirrorEditorRuntime({ parent, ...props });
    const view = EditorView.findFromDOM(
      parent.querySelector<HTMLElement>(".cm-content")!,
    )!;
    // The editing features and the language load lazily: wait for the
    // imports, not a clock.
    await settleLazyModules();
    expect(view.dom.querySelector(".cm-foldGutter")).not.toBeNull();
    const toString = vi.spyOn(Text.prototype, "toString");
    for (let index = 0; index < KEYSTROKES; index += 1) {
      // Typing inside brackets, beside a repeated word: every feature runs.
      view.dispatch({
        changes: { from: 5 + index, insert: "x" },
        selection: { anchor: 6 + index },
        userEvent: "input.type",
      });
      runtime.update({
        ...props,
        content: onChange.mock.lastCall?.[0],
        documentIdentity: `big\0${index + 1}`,
      });
    }
    expect(toString).not.toHaveBeenCalled();
    expect(onChange).toHaveBeenCalledTimes(KEYSTROKES);
    runtime.destroy();
  });

  it("says whether an edited document still equals the text it was given", () => {
    const { runtime, view, onChange } = mount("a".repeat(100_000), 1 << 20);
    const base = (call: number) => onChange.mock.calls[call][1];
    view.dispatch({ changes: { from: 50_000, insert: "b" } });
    expect(base(0)).toMatchObject({ unchanged: false });
    view.dispatch({ changes: { from: 50_000, to: 50_001 } });
    expect(base(1)).toMatchObject({ unchanged: true });
    // Same length, different text.
    view.dispatch({ changes: { from: 7, to: 8, insert: "c" } });
    expect(base(2)).toMatchObject({ unchanged: false });
    expect(base(2).content).toHaveLength(100_000);
    runtime.destroy();

    // A document handed back from the store has no string to compare with.
    const remount = mount(Text.of(["x"]), 1024);
    remount.view.dispatch({ changes: { from: 1, insert: "y" } });
    expect(remount.onChange.mock.calls[0][1]).toBeNull();
    remount.runtime.destroy();
  });

  it("re-bases on a reloaded string and keeps the base across a remount", () => {
    const { runtime, view, onChange } = mount("one\n", 1024);
    const reload = {
      ariaLabel: "Edit big.txt",
      path: "big.txt",
      content: "two\n",
      documentIdentity: "big\0reloaded",
      disabled: false,
      maxContentLength: 1024,
      onChange,
    };
    runtime.update({ ...reload, documentIdentity: "big\0loaded" });
    runtime.update(reload);
    expect(view.state.doc.toString()).toBe("two\n");
    view.dispatch({ changes: { from: 0, insert: "x" } });
    view.dispatch({ changes: { from: 0, to: 1 } });
    expect(onChange.mock.lastCall?.[1]).toEqual({
      content: "two\n",
      unchanged: true,
    });
    view.dispatch({ changes: { from: 0, insert: "y" } });
    const emitted = onChange.mock.lastCall?.[0];
    runtime.destroy();

    // Remounted on the store's document, it still knows the reloaded text.
    const remount = mount(emitted, 1024);
    remount.view.dispatch({ changes: { from: 0, to: 1 } });
    expect(remount.onChange.mock.lastCall?.[1]).toEqual({
      content: "two\n",
      unchanged: true,
    });
    remount.runtime.destroy();
  });

  it("does not claim a base for text with a lone carriage return", () => {
    // CodeMirror turns the "\r" into a line break, so its document is not
    // the given text; the controller must compare the text itself.
    const { runtime, view, onChange } = mount("a\rb", 1024);
    view.dispatch({ changes: { from: 0, insert: "x" } });
    expect(onChange.mock.lastCall?.[1]).toBeNull();
    runtime.destroy();
  });

  it("still refuses an edit that would cross the byte limit", () => {
    const { runtime, view, onChange } = mount("é".repeat(400), 802);
    view.dispatch({ changes: { from: 0, insert: "a" } });
    view.dispatch({ changes: { from: 0, insert: "é" } });
    expect(view.state.doc.length).toBe(401);
    expect(onChange).toHaveBeenCalledTimes(1);
    runtime.destroy();
  });
});
