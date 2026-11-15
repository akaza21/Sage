import type { EditorView } from "@codemirror/view";

/** Lives in its own chunk so the File surface carries only the registration. */
const editors = new Map<string, EditorView>();

export function registerReplaceEditor(key: string, view: EditorView) {
  editors.set(key, view);
  return () => {
    if (editors.get(key) === view) editors.delete(key);
  };
}

export function replaceEditorText(
  key: string,
  expected: string,
  content: string,
): "applied" | "missing" | "changed" {
  const view = editors.get(key);
  if (!view) return "missing";
  const text = view.state.doc.toString();
  if (text !== expected) return "changed";
  const shorter = Math.min(text.length, content.length);
  let from = 0;
  while (from < shorter && text[from] === content[from]) from += 1;
  let tail = 0;
  while (
    tail < shorter - from &&
    text[text.length - 1 - tail] === content[content.length - 1 - tail]
  ) {
    tail += 1;
  }
  view.dispatch({
    changes: {
      from,
      to: text.length - tail,
      insert: content.slice(from, content.length - tail),
    },
    userEvent: "input.replace",
  });
  return "applied";
}
