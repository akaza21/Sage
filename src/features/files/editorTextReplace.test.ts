import { history, undo } from "@codemirror/commands";
import { EditorView } from "@codemirror/view";
import { describe, expect, it } from "vitest";

import { registerReplaceEditor, replaceEditorText } from "./editorTextReplace";

describe("editor text replace", () => {
  it("replaces only what differs, as one undoable edit", () => {
    const view = new EditorView({
      doc: "aaa oldName bbb",
      extensions: history(),
    });
    view.dispatch({ selection: { anchor: 14 } });
    const unregister = registerReplaceEditor("file-a", view);
    expect(
      replaceEditorText("file-a", "aaa oldName bbb", "aaa newName bbb"),
    ).toBe("applied");
    expect(view.state.doc.toString()).toBe("aaa newName bbb");
    expect(view.state.selection.main.head).toBe(14);
    undo(view);
    expect(view.state.doc.toString()).toBe("aaa oldName bbb");
    unregister();
    view.destroy();
  });

  it("leaves a changed text alone and reports an editor that is gone", () => {
    const view = new EditorView({ doc: "edited" });
    const unregister = registerReplaceEditor("file-a", view);
    expect(replaceEditorText("file-a", "planned", "next")).toBe("changed");
    expect(view.state.doc.toString()).toBe("edited");
    expect(replaceEditorText("file-b", "edited", "next")).toBe("missing");
    unregister();
    expect(replaceEditorText("file-a", "edited", "next")).toBe("missing");
    view.destroy();
  });

  it("keeps a newer editor registered when an older one unregisters", () => {
    const older = new EditorView({ doc: "x" });
    const newer = new EditorView({ doc: "x" });
    const unregisterOlder = registerReplaceEditor("file-a", older);
    const unregisterNewer = registerReplaceEditor("file-a", newer);
    unregisterOlder();
    expect(replaceEditorText("file-a", "x", "y")).toBe("applied");
    expect(newer.state.doc.toString()).toBe("y");
    unregisterNewer();
    older.destroy();
    newer.destroy();
  });
});
