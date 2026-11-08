import { RangeSetBuilder, StateEffect, StateField } from "@codemirror/state";
import { Decoration, EditorView, type DecorationSet } from "@codemirror/view";

// Find and Replace marks. The field maps them through edits, so they
// stay on their text while a new search waits for a typing pause.
const setMarks = StateEffect.define<DecorationSet>();
const marks = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update: (value, transaction) => {
    // Marks set by this transaction replace the old ones unmapped: mapping
    // walks the changes once per mark, so a Replace all that also clears
    // the marks must not first map thousands of marks through thousands
    // of changes.
    let next: DecorationSet | null = null;
    for (const effect of transaction.effects)
      if (effect.is(setMarks)) next = effect.value;
    return next ?? value.map(transaction.changes);
  },
  provide: (field) => EditorView.decorations.from(field),
});

/** Drops every mark in the transaction that carries it, as Replace all does. */
export const clearMatches = () => setMarks.of(Decoration.none);
const MATCH = Decoration.mark({ class: "cm-findMatch" });
const CURRENT = Decoration.mark({
  class: "cm-findMatch cm-findMatch--current",
});
const marked = new WeakSet<EditorView>();

/** Marks every match, the current one more strongly. CodeMirror draws only the visible ones. */
export function showMatches(
  view: EditorView | null | undefined,
  ranges: readonly number[],
  current: number,
) {
  if (!view) return;
  if (!marked.has(view)) {
    marked.add(view);
    view.dispatch({ effects: StateEffect.appendConfig.of(marks) });
  }
  const builder = new RangeSetBuilder<Decoration>();
  const length = view.state.doc.length;
  for (let index = 0; index + 1 < ranges.length; index += 2) {
    if (ranges[index + 1]! > length) break;
    builder.add(
      ranges[index]!,
      ranges[index + 1]!,
      index === current * 2 ? CURRENT : MATCH,
    );
  }
  view.dispatch({ effects: setMarks.of(builder.finish()) });
}
