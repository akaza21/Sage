import { RangeSetBuilder, StateEffect, StateField } from "@codemirror/state";
import {
  Decoration,
  ViewPlugin,
  type DecorationSet,
  type EditorView,
  type ViewUpdate,
} from "@codemirror/view";

// Marks map through edits so they stay on their text while a search waits.
const setMarks = StateEffect.define<DecorationSet>();
const marks = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update: (value, transaction) => {
    // Marks set by this transaction replace the old ones unmapped, so a Replace
    // all never maps thousands of marks through thousands of changes.
    let next: DecorationSet | null = null;
    for (const effect of transaction.effects)
      if (effect.is(setMarks)) next = effect.value;
    return next ?? value.map(transaction.changes);
  },
});

export const VISIBLE_MARK_MARGIN = 250;

function visibleMarks(view: EditorView): DecorationSet {
  const all = view.state.field(marks);
  const builder = new RangeSetBuilder<Decoration>();
  let next = 0;
  for (const { from, to } of view.visibleRanges)
    all.between(
      from - VISIBLE_MARK_MARGIN,
      to + VISIBLE_MARK_MARGIN,
      (start, end, mark) => {
        // Margins can overlap: a mark that starts before the last one added
        // was added already.
        if (start < next) return;
        builder.add(start, end, mark);
        next = start + 1;
      },
    );
  return builder.finish();
}

const drawMarks = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;
    constructor(view: EditorView) {
      this.decorations = visibleMarks(view);
    }
    update(update: ViewUpdate) {
      if (
        update.docChanged ||
        update.viewportChanged ||
        update.state.field(marks) !== update.startState.field(marks)
      )
        this.decorations = visibleMarks(update.view);
    }
  },
  { decorations: (plugin) => plugin.decorations },
);

export const clearMatches = () => setMarks.of(Decoration.none);
const MATCH = Decoration.mark({ class: "cm-findMatch" });
const CURRENT = Decoration.mark({
  class: "cm-findMatch cm-findMatch--current",
});
const marked = new WeakSet<EditorView>();

export function showMatches(
  view: EditorView | null | undefined,
  ranges: readonly number[],
  current: number,
) {
  if (!view) return;
  if (!marked.has(view)) {
    marked.add(view);
    view.dispatch({ effects: StateEffect.appendConfig.of([marks, drawMarks]) });
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
