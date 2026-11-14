import { javascript } from "@codemirror/lang-javascript";
import { foldedRanges } from "@codemirror/language";
import { EditorSelection, EditorState } from "@codemirror/state";
import { EditorView, runScopeHandlers } from "@codemirror/view";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  KEYMAP_BINDINGS,
  matchesKeymapChord,
  matchesPlatformChord,
  type KeymapBinding,
} from "../../domain/keymapRegistry";
import { EDITOR_EDITING_BINDINGS } from "../../domain/settingsNavigation";
import type { EditorEditing } from "./editorAppearance";
import {
  EDITING_KEYS,
  editingExtensions,
  FOLD_ARROW_DELAY_MS,
  OCCURRENCE_DELAY_MS,
} from "./editorEditing";

const ALL_ON: EditorEditing = {
  matchBrackets: true,
  autoClosingBrackets: true,
  folding: true,
  occurrencesHighlight: true,
  multiCursorModifier: "alt",
};

afterEach(() => {
  vi.useRealTimers();
  document.body.replaceChildren();
});

/** Lines that show an open fold arrow (the hidden sizing spacer is closed). */
function openArrows(view: EditorView) {
  return view.dom.querySelectorAll('.cm-foldMarker[data-open="true"]').length;
}

function mount(doc: string, editing: Partial<EditorEditing> = {}) {
  const parent = document.createElement("div");
  document.body.append(parent);
  return new EditorView({
    parent,
    state: EditorState.create({
      doc,
      extensions: [javascript(), editingExtensions({ ...ALL_ON, ...editing })],
    }),
  });
}

/** Runs a key the way CodeMirror's keydown handler does. */
function press(
  view: EditorView,
  key: string,
  modifiers: KeyboardEventInit = {},
) {
  return runScopeHandlers(
    view,
    new KeyboardEvent("keydown", { key, ...modifiers }),
    "editor",
  );
}

function type(view: EditorView, text: string) {
  const { from, to } = view.state.selection.main;
  const handled = view.state
    .facet(EditorView.inputHandler)
    .some((handler) =>
      handler(view, from, to, text, () =>
        view.state.update({ changes: { from, to, insert: text } }),
      ),
    );
  if (!handled) {
    view.dispatch({
      changes: { from, to, insert: text },
      selection: { anchor: from + text.length },
    });
  }
}

function chordEvent(binding: KeymapBinding, apple: boolean) {
  const { shift, alt } = (apple && binding.apple) || binding;
  const primary = binding.ctrlOrMeta === "required";
  return {
    key: binding.key,
    ctrlKey: primary && !apple,
    metaKey: primary && apple,
    shiftKey: shift === "required",
    altKey: alt === "required",
  };
}

function codeMirrorName(binding: KeymapBinding, apple: boolean) {
  const { shift, alt } = (apple && binding.apple) || binding;
  const names: Record<string, string> = {
    tab: "Tab",
    arrowup: "ArrowUp",
    arrowdown: "ArrowDown",
  };
  return [
    binding.ctrlOrMeta === "required" ? "Mod" : null,
    shift === "required" ? "Shift" : null,
    alt === "required" ? "Alt" : null,
    names[binding.key] ?? binding.key,
  ]
    .filter(Boolean)
    .join("-");
}

describe("editing shortcuts", () => {
  it("bind exactly the chords the Keyboard settings page lists", () => {
    expect(
      EDITING_KEYS.map(({ id, key, mac }) => ({ id, key, mac: mac ?? key })),
    ).toEqual(
      EDITOR_EDITING_BINDINGS.map((binding) => ({
        id: binding.id,
        key: codeMirrorName(binding, false),
        mac: codeMirrorName(binding, true),
      })),
    );
  });

  it("never reuse a chord a Sage shortcut already answers, on either platform", () => {
    const live = KEYMAP_BINDINGS.filter(
      (binding) =>
        !binding.reservedFor &&
        (binding.scope === "Global" || binding.scope === "Editor"),
    );
    const clashes: string[] = [];
    for (const editing of EDITOR_EDITING_BINDINGS) {
      for (const apple of [false, true]) {
        const event = chordEvent(editing, apple);
        for (const binding of live) {
          if (
            matchesPlatformChord(binding, event, apple) ||
            matchesKeymapChord(binding, event)
          ) {
            clashes.push(`${editing.id} vs ${binding.id} (apple: ${apple})`);
          }
        }
      }
    }
    expect(clashes).toEqual([]);
    for (const editing of EDITOR_EDITING_BINDINGS) {
      expect(
        matchesPlatformChord(
          KEYMAP_BINDINGS.find((binding) => binding.id === "quickOpen")!,
          chordEvent(editing, false),
          false,
        ),
      ).toBe(false);
    }
  });
});

describe("editing features", () => {
  it("auto-closes brackets and quotes, and Backspace removes the pair", () => {
    const view = mount("");
    type(view, "(");
    expect(view.state.doc.toString()).toBe("()");
    type(view, '"');
    expect(view.state.doc.toString()).toBe('("")');
    expect(press(view, "Backspace")).toBe(true);
    expect(view.state.doc.toString()).toBe("()");
    view.destroy();
  });

  it("leaves brackets alone when auto-close is off", () => {
    const view = mount("", { autoClosingBrackets: false });
    type(view, "(");
    expect(view.state.doc.toString()).toBe("(");
    view.destroy();
  });

  it("marks the matching bracket only when bracket matching is on", () => {
    const on = mount("call(a, b)");
    on.dispatch({ selection: { anchor: 4 } });
    expect(on.dom.querySelectorAll(".cm-matchingBracket")).toHaveLength(2);
    const off = mount("call(a, b)", { matchBrackets: false });
    off.dispatch({ selection: { anchor: 4 } });
    expect(off.dom.querySelector(".cm-matchingBracket")).toBeNull();
    on.destroy();
    off.destroy();
  });

  it("folds and unfolds a block from the keyboard, with a fold gutter", () => {
    const view = mount("function f() {\n  return 1;\n}\n");
    expect(view.dom.querySelector(".cm-foldGutter")).not.toBeNull();
    view.dispatch({ selection: { anchor: 3 } });
    expect(
      press(view, "{", { ctrlKey: true, shiftKey: true, keyCode: 219 }),
    ).toBe(true);
    expect(foldedRanges(view.state).size).toBe(1);
    expect(
      press(view, "}", { ctrlKey: true, shiftKey: true, keyCode: 221 }),
    ).toBe(true);
    expect(foldedRanges(view.state).size).toBe(0);
    view.destroy();
  });

  it("has no fold gutter or fold keys when folding is off", () => {
    const view = mount("function f() {\n  return 1;\n}\n", { folding: false });
    expect(view.dom.querySelector(".cm-foldGutter")).toBeNull();
    view.dispatch({ selection: { anchor: 3 } });
    expect(
      press(view, "{", { ctrlKey: true, shiftKey: true, keyCode: 219 }),
    ).toBe(false);
    view.destroy();
  });

  it("indents with Tab and outdents with Shift+Tab", () => {
    const view = mount("a\nb");
    view.dispatch({ selection: EditorSelection.range(0, 3) });
    expect(press(view, "Tab")).toBe(true);
    expect(view.state.doc.toString()).toBe("  a\n  b");
    expect(press(view, "Tab", { shiftKey: true })).toBe(true);
    expect(view.state.doc.toString()).toBe("a\nb");
    view.destroy();
  });

  it("keeps Tab in the editor while a save briefly locks it", () => {
    const parent = document.createElement("div");
    document.body.append(parent);
    const view = new EditorView({
      parent,
      state: EditorState.create({
        doc: "a",
        extensions: [EditorState.readOnly.of(true), editingExtensions(ALL_ON)],
      }),
    });
    expect(press(view, "Tab")).toBe(true);
    expect(view.state.doc.toString()).toBe("a");
    view.destroy();
  });

  it("lets Esc then Tab leave the editor instead of indenting", () => {
    const view = mount("a");
    view.contentDOM.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Escape",
        keyCode: 27,
        bubbles: true,
      }),
    );
    const tab = new KeyboardEvent("keydown", {
      key: "Tab",
      keyCode: 9,
      bubbles: true,
      cancelable: true,
    });
    view.contentDOM.dispatchEvent(tab);
    expect(tab.defaultPrevented).toBe(false);
    expect(view.state.doc.toString()).toBe("a");
    view.destroy();
  });

  it("adds the next occurrence with Ctrl+D", () => {
    const view = mount("item + item + item");
    view.dispatch({ selection: EditorSelection.range(0, 4) });
    expect(press(view, "d", { ctrlKey: true })).toBe(true);
    expect(view.state.selection.ranges).toHaveLength(2);
    expect(press(view, "d", { ctrlKey: true })).toBe(true);
    expect(view.state.selection.ranges).toHaveLength(3);
    view.destroy();
  });

  it("draws its own cursors only while there are several", () => {
    const view = mount("a\nb");
    expect(view.dom.querySelector(".cm-cursorLayer")).toBeNull();
    view.dispatch({
      selection: EditorSelection.create([
        EditorSelection.cursor(1),
        EditorSelection.cursor(3),
      ]),
    });
    expect(view.dom.querySelector(".cm-cursorLayer")).not.toBeNull();
    view.dispatch({ selection: { anchor: 0 } });
    expect(view.dom.querySelector(".cm-cursorLayer")).toBeNull();
    view.destroy();
  });

  it("keeps several cursors and types at each", () => {
    const view = mount("a\nb");
    view.dispatch({
      selection: EditorSelection.create([
        EditorSelection.cursor(1),
        EditorSelection.cursor(3),
      ]),
    });
    expect(view.state.selection.ranges).toHaveLength(2);
    view.dispatch(view.state.replaceSelection("!"));
    expect(view.state.doc.toString()).toBe("a!\nb!");
    view.destroy();
  });

  it("adds a cursor with the configured click modifier", () => {
    const click = (init: MouseEventInit) => new MouseEvent("mousedown", init);
    const alt = mount("a");
    const [altRule] = alt.state.facet(EditorView.clickAddsSelectionRange);
    expect(altRule(click({ altKey: true }))).toBe(true);
    expect(altRule(click({ ctrlKey: true }))).toBe(false);
    const ctrl = mount("a", { multiCursorModifier: "ctrlCmd" });
    const [ctrlRule] = ctrl.state.facet(EditorView.clickAddsSelectionRange);
    expect(ctrlRule(click({ ctrlKey: true }))).toBe(true);
    expect(ctrlRule(click({ altKey: true }))).toBe(false);
    alt.destroy();
    ctrl.destroy();
  });

  it("toggles line and block comments in the file's comment syntax", () => {
    const view = mount("let a = 1;");
    expect(press(view, "/", { ctrlKey: true })).toBe(true);
    expect(view.state.doc.toString()).toBe("// let a = 1;");
    expect(press(view, "/", { ctrlKey: true })).toBe(true);
    expect(view.state.doc.toString()).toBe("let a = 1;");
    view.dispatch({ selection: EditorSelection.range(4, 5) });
    expect(
      press(view, "A", { shiftKey: true, altKey: true, keyCode: 65 }),
    ).toBe(true);
    expect(view.state.doc.toString()).toBe("let /* a */ = 1;");
    view.destroy();
  });

  it("highlights other copies of the word at the cursor only when asked", () => {
    const on = mount("item + item + item");
    on.dispatch({ selection: { anchor: 1 } });
    expect(
      on.dom.querySelectorAll(
        ".cm-selectionMatch:not(.cm-selectionMatch-main)",
      ),
    ).toHaveLength(2);
    const off = mount("item + item + item", { occurrencesHighlight: false });
    off.dispatch({ selection: { anchor: 1 } });
    expect(off.dom.querySelector(".cm-selectionMatch")).toBeNull();
    on.destroy();
    off.destroy();
  });

  it("folds and unfolds from the gutter arrow", () => {
    const view = mount("function f() {\n  return 1;\n}\n");
    expect(openArrows(view)).toBe(1);
    const arrow = view.dom.querySelector<HTMLElement>(
      '.cm-foldMarker[data-open="true"]',
    )!;
    arrow.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(foldedRanges(view.state).size).toBe(1);
    expect(openArrows(view)).toBe(0);
    view.dom
      .querySelector<HTMLElement>(
        '.cm-gutterElement:not([style*="hidden"]) .cm-foldMarker',
      )!
      .dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(foldedRanges(view.state).size).toBe(0);
    view.destroy();
  });

  it("moves fold arrows with the text while typing, and refreshes them after a pause", () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    const view = mount("let a = 1;\nfunction f() {\n  return 1;\n}\n");
    expect(openArrows(view)).toBe(1);
    view.dispatch({ changes: { from: 0, insert: "if (a) {\n  a();\n}\n" } });
    expect(openArrows(view)).toBe(1);
    vi.advanceTimersByTime(FOLD_ARROW_DELAY_MS - 1);
    expect(openArrows(view)).toBe(1);
    vi.advanceTimersByTime(1);
    expect(openArrows(view)).toBe(2);
    view.destroy();
  });

  it("redraws fold arrows at once when an edit deletes a fold", () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    const block = "function f() {\n  return 1;\n}\n";
    const view = mount(block + "const x = 1;\n");
    view.dispatch({ selection: { anchor: 3 } });
    expect(
      press(view, "{", { ctrlKey: true, shiftKey: true, keyCode: 219 }),
    ).toBe(true);
    const closed = () =>
      view.dom.querySelectorAll(
        '.cm-gutterElement:not([style*="hidden"]) .cm-foldMarker[data-open="false"]',
      ).length;
    expect(closed()).toBe(1);
    view.dispatch({ changes: { from: 0, to: block.length } });
    expect(foldedRanges(view.state).size).toBe(0);
    expect(closed()).toBe(0);
    view.destroy();
  });

  it("clears occurrence highlights while typing and restores them after a pause", () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    const view = mount("item + item + item");
    view.dispatch({ selection: { anchor: 1 } });
    const others = () =>
      view.dom.querySelectorAll(
        ".cm-selectionMatch:not(.cm-selectionMatch-main)",
      ).length;
    expect(others()).toBe(2);
    view.dispatch({ changes: { from: 18, insert: " + item" } });
    expect(others()).toBe(0);
    view.dispatch({ selection: { anchor: 2 } });
    expect(others()).toBe(0);
    vi.advanceTimersByTime(OCCURRENCE_DELAY_MS);
    expect(others()).toBe(3);
    view.destroy();
  });

  it("stops its timers when the editor closes", () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    const view = mount("item + item");
    view.dispatch({ changes: { from: 0, insert: "x" } });
    expect(vi.getTimerCount()).toBe(2);
    view.destroy();
    expect(vi.getTimerCount()).toBe(0);
  });
});
