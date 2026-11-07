/**
 * Editing features of an editable File, loaded on demand for editable
 * Files only, so read-only views and Agent chat code blocks never load them.
 * Each has its own flag so large mode can turn costly ones off. Esc
 * then Tab still leaves the editor: CodeMirror frees Tab for 2 s after Escape.
 *
 * Like VS Code, typing neither asks the syntax tree nor scans the visible
 * text: fold arrows move with the text and refresh once typing pauses, and
 * occurrence highlights return after a delay. CodeMirror's fold gutter and
 * match highlighter redo both on every change, doubling a keystroke's cost in
 * a large File.
 */
import { closeBrackets, closeBracketsKeymap } from "@codemirror/autocomplete";
import {
  addCursorAbove,
  addCursorBelow,
  indentLess,
  indentMore,
  toggleBlockComment,
  toggleComment,
} from "@codemirror/commands";
import {
  bracketMatching,
  codeFolding,
  foldable,
  foldCode,
  foldedRanges,
  foldEffect,
  language,
  syntaxTree,
  unfoldCode,
  unfoldEffect,
} from "@codemirror/language";
import {
  highlightSelectionMatches,
  selectNextOccurrence,
} from "@codemirror/search";
import {
  Compartment,
  EditorState,
  Prec,
  RangeSetBuilder,
  StateEffect,
  RangeSet,
  type Extension,
  type StateEffectType,
} from "@codemirror/state";
import {
  drawSelection,
  EditorView,
  gutter,
  GutterMarker,
  keymap,
  ViewPlugin,
  type KeyBinding,
  type ViewUpdate,
} from "@codemirror/view";

import { usesMetaModifier } from "../../ui/shortcutLabel";
import type { EditorEditing } from "./editorAppearance";

/**
 * The editor's own shortcuts, by the id the Keyboard settings page lists them
 * under (`EDITOR_EDITING_BINDINGS`); a test keeps the chords equal. `Mod` is
 * Command on macOS, Control elsewhere; `mac` is set where macOS differs.
 */
export const EDITING_KEYS: readonly (KeyBinding & {
  id: string;
  key: string;
})[] = [
  {
    id: "editor.indent",
    key: "Tab",
    run: indentMore,
    shift: indentLess,
  },
  {
    id: "editor.selectNextOccurrence",
    key: "Mod-d",
    run: selectNextOccurrence,
  },
  {
    id: "editor.addCursorAbove",
    key: "Mod-Alt-ArrowUp",
    run: addCursorAbove,
  },
  {
    id: "editor.addCursorBelow",
    key: "Mod-Alt-ArrowDown",
    run: addCursorBelow,
  },
  {
    id: "editor.toggleLineComment",
    key: "Mod-/",
    run: toggleComment,
  },
  {
    id: "editor.toggleBlockComment",
    key: "Shift-Alt-a",
    run: toggleBlockComment,
  },
  {
    id: "editor.fold",
    key: "Mod-Shift-[",
    mac: "Mod-Alt-[",
    run: foldCode,
  },
  {
    id: "editor.unfold",
    key: "Mod-Shift-]",
    mac: "Mod-Alt-]",
    run: unfoldCode,
  },
  // The browser's own Ctrl+D and the like stay off even when a command has
  // nothing to do. Tab too: read-only Files get none of these keys, so the
  // editor cannot indent only while a save or Project switch briefly locks it.
].map((binding) => ({ ...binding, preventDefault: true }));

/**
 * On macOS Shift+Option+A types "Å", and CodeMirror matches Option chords by
 * the typed character, so the block comment chord goes by the physical key.
 */
const macBlockComment: KeyBinding = {
  any: (view, event) => {
    const chord =
      event.code === "KeyA" &&
      event.shiftKey &&
      event.altKey &&
      !event.metaKey &&
      !event.ctrlKey &&
      usesMetaModifier();
    // Handled even without block comment syntax, so "Å" is never typed.
    if (chord) toggleBlockComment(view);
    return chord;
  },
};

/** VS Code recomputes folding ranges once typing pauses. */
export const FOLD_ARROW_DELAY_MS = 200;
/** VS Code's `editor.occurrencesHighlightDelay` default. */
export const OCCURRENCE_DELAY_MS = 250;

const refreshFoldArrows = StateEffect.define<null>();

function hasEffect(update: ViewUpdate, type: StateEffectType<null>) {
  return update.transactions.some((transaction) =>
    transaction.effects.some((effect) => effect.is(type)),
  );
}

/** Dispatches `effect()` once `delay` ms pass without another `schedule`. */
function pause(
  view: EditorView,
  delay: number,
  effect: () => StateEffect<unknown>,
) {
  let timer: ReturnType<typeof setTimeout> | null = null;
  const cancel = () => {
    if (timer !== null) clearTimeout(timer);
    timer = null;
  };
  return {
    pending: () => timer !== null,
    cancel,
    schedule() {
      cancel();
      timer = setTimeout(() => {
        timer = null;
        view.dispatch({ effects: effect() });
      }, delay);
    },
  };
}

/** The fold arrow: a chevron drawn in CSS, pointing down while open. */
class FoldArrow extends GutterMarker {
  constructor(readonly open: boolean) {
    super();
  }

  eq(other: GutterMarker) {
    return other instanceof FoldArrow && other.open === this.open;
  }

  toDOM() {
    const marker = document.createElement("span");
    marker.className = "cm-foldMarker";
    marker.dataset.open = String(this.open);
    return marker;
  }
}

const CAN_FOLD = new FoldArrow(true);
const CAN_UNFOLD = new FoldArrow(false);

function foldOnLine(state: EditorState, from: number, to: number) {
  let found: { from: number; to: number } | null = null;
  foldedRanges(state).between(from, to, (start, end) => {
    if (!found || found.from > start) found = { from: start, to: end };
  });
  return found as { from: number; to: number } | null;
}

function buildArrows(view: EditorView) {
  const builder = new RangeSetBuilder<GutterMarker>();
  for (const line of view.viewportLineBlocks) {
    const arrow = foldOnLine(view.state, line.from, line.to)
      ? CAN_UNFOLD
      : foldable(view.state, line.from, line.to)
        ? CAN_FOLD
        : null;
    if (arrow) builder.add(line.from, line.from, arrow);
  }
  return builder.finish();
}

const foldArrows = ViewPlugin.fromClass(
  class {
    arrows: RangeSet<GutterMarker>;
    private readonly pause: ReturnType<typeof pause>;

    constructor(view: EditorView) {
      this.arrows = buildArrows(view);
      this.pause = pause(view, FOLD_ARROW_DELAY_MS, () =>
        refreshFoldArrows.of(null),
      );
    }

    update(update: ViewUpdate) {
      if (update.docChanged && !foldsEdited(update)) {
        this.arrows = this.arrows.map(update.changes);
        this.pause.schedule();
      } else if (
        update.docChanged ||
        hasEffect(update, refreshFoldArrows) ||
        update.viewportChanged ||
        foldedRanges(update.startState) !== foldedRanges(update.state)
      ) {
        this.arrows = buildArrows(update.view);
      } else if (!this.pause.pending() && treeMoved(update)) {
        this.pause.schedule();
      }
    }

    destroy() {
      this.pause.cancel();
    }
  },
);

/** An edit that also folds, unfolds or deletes a fold redraws at once. */
function foldsEdited(update: ViewUpdate) {
  return (
    foldedRanges(update.startState).size !== foldedRanges(update.state).size ||
    update.transactions.some((transaction) =>
      transaction.effects.some(
        (effect) => effect.is(foldEffect) || effect.is(unfoldEffect),
      ),
    )
  );
}

/** A new parse, or a new language, can add or remove fold ranges. */
function treeMoved(update: ViewUpdate) {
  return (
    syntaxTree(update.startState) !== syntaxTree(update.state) ||
    update.startState.facet(language) !== update.state.facet(language)
  );
}

/** The fold gutter: arrows from `foldArrows`, a click folds or unfolds. */
function foldGutter(): Extension {
  return [
    foldArrows,
    // Low precedence puts the arrows after the line numbers, as in VS Code.
    Prec.low(
      gutter({
        class: "cm-foldGutter",
        markers: (view) => view.plugin(foldArrows)?.arrows ?? RangeSet.empty,
        initialSpacer: () => CAN_UNFOLD,
        domEventHandlers: {
          click: (view, line) => {
            const folded = foldOnLine(view.state, line.from, line.to);
            if (folded) {
              view.dispatch({ effects: unfoldEffect.of(folded) });
              return true;
            }
            const range = foldable(view.state, line.from, line.to);
            if (!range) return false;
            view.dispatch({ effects: foldEffect.of(range) });
            return true;
          },
        },
      }),
    ),
    codeFolding(),
  ];
}

/**
 * CodeMirror's match highlighter rescans the visible text on every change, so
 * the first keystroke switches it off and it returns once typing pauses.
 */
const occurrenceSlot = new Compartment();
const OCCURRENCES = highlightSelectionMatches({
  highlightWordAroundCursor: true,
});
const occurrences: Extension = [
  occurrenceSlot.of(OCCURRENCES),
  EditorState.transactionExtender.of((transaction) =>
    transaction.docChanged &&
    occurrenceSlot.get(transaction.startState) === OCCURRENCES
      ? { effects: occurrenceSlot.reconfigure([]) }
      : null,
  ),
  ViewPlugin.fromClass(
    class {
      private readonly pause: ReturnType<typeof pause>;

      constructor(view: EditorView) {
        this.pause = pause(view, OCCURRENCE_DELAY_MS, () =>
          occurrenceSlot.reconfigure(OCCURRENCES),
        );
      }

      update(update: ViewUpdate) {
        if (update.docChanged) this.pause.schedule();
      }

      destroy() {
        this.pause.cancel();
      }
    },
  ),
];

/**
 * CodeMirror's drawn cursors cost about 3 ms a keystroke in a 1 MiB File, so
 * a single cursor stays the browser's own caret, as it was before these features, and
 * the drawn layer joins only while there are several cursors to show.
 */
const cursorDrawing = new Compartment();
const DRAWN_CURSORS = drawSelection();
const drawCursorsWhenSeveral = EditorState.transactionExtender.of(
  (transaction) => {
    const several = transaction.newSelection.ranges.length > 1;
    const drawn = cursorDrawing.get(transaction.startState) === DRAWN_CURSORS;
    if (several === drawn) return null;
    return {
      effects: cursorDrawing.reconfigure(several ? DRAWN_CURSORS : []),
    };
  },
);

export function editingExtensions(editing: EditorEditing): Extension {
  const modifier = editing.multiCursorModifier;
  return [
    EditorState.allowMultipleSelections.of(true),
    cursorDrawing.of([]),
    drawCursorsWhenSeveral,
    EditorView.clickAddsSelectionRange.of((event) =>
      modifier === "alt"
        ? event.altKey
        : usesMetaModifier()
          ? event.metaKey
          : event.ctrlKey,
    ),
    editing.matchBrackets ? bracketMatching() : [],
    editing.autoClosingBrackets
      ? [closeBrackets(), keymap.of(closeBracketsKeymap)]
      : [],
    editing.folding ? foldGutter() : [],
    editing.occurrencesHighlight ? occurrences : [],
    keymap.of([
      macBlockComment,
      ...EDITING_KEYS.filter(
        (binding) =>
          editing.folding ||
          (binding.id !== "editor.fold" && binding.id !== "editor.unfold"),
      ).map(({ id: _id, ...binding }) => binding),
    ]),
  ];
}
