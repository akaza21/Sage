import { isolateHistory } from "@codemirror/commands";
import {
  Compartment,
  Prec,
  RangeSetBuilder,
  StateEffect,
  type ChangeSet,
  type EditorState,
  type Extension,
  type Text,
} from "@codemirror/state";
import {
  Decoration,
  EditorView,
  GutterMarker,
  ViewPlugin,
  gutter,
  keymap,
  type DecorationSet,
  type ViewUpdate,
} from "@codemirror/view";

import { GitGutterPeek, type PeekActions } from "./gitGutterPeek";
import {
  gutterField,
  hunkAtLine,
  hunksFromLines,
  revertChange,
  setGutterBase,
  setGutterHunks,
  setGutterPeek,
  stagedText,
  type GutterState,
} from "./gitGutterState";
import type { GutterConnection } from "./gutterWorkerClient";
import type { GutterResult } from "./gutterWorkerCore";

/** AD-7: the diff waits for this long a pause in typing. */
export const GUTTER_IDLE_MS = 250;

export interface GutterPool {
  connect: (listener: (result: GutterResult) => void) => GutterConnection;
}

class ChangeMarker extends GutterMarker {
  constructor(private readonly kind: string) {
    super();
  }
  override eq(other: ChangeMarker) {
    return other.kind === this.kind;
  }
  override toDOM() {
    const element = document.createElement("div");
    element.className = `git-gutter-marker git-gutter-marker--${this.kind}`;
    return element;
  }
}

const MARKERS = {
  added: new ChangeMarker("added"),
  modified: new ChangeMarker("modified"),
  deletedAbove: new ChangeMarker("deleted-above"),
  deletedBelow: new ChangeMarker("deleted-below"),
};

function markerAt(
  state: GutterState,
  from: number,
  to: number,
  length: number,
) {
  const index = hunkAtLine(state.hunks, from, to, length);
  if (index === null) return null;
  const hunk = state.hunks[index]!;
  if (hunk.kind !== "deleted") return MARKERS[hunk.kind];
  return hunk.atEnd ? MARKERS.deletedBelow : MARKERS.deletedAbove;
}

function openPeek(view: EditorView, index: number | null) {
  view.dispatch({
    effects: setGutterPeek.of(index),
    ...(index === null
      ? {}
      : {
          selection: {
            anchor: view.state.field(gutterField).hunks[index]!.from,
          },
          scrollIntoView: true,
        }),
  });
}

function step(view: EditorView, delta: number) {
  const { hunks, peek } = view.state.field(gutterField);
  if (hunks.length === 0) return;
  const index = ((peek ?? -1) + delta + hunks.length) % hunks.length;
  openPeek(view, index);
}

/** Puts the staged lines back in one undo step. */
export function revertHunk(view: EditorView, index: number): boolean {
  const state = view.state.field(gutterField);
  const hunk = state.hunks[index];
  if (!hunk || state.base === null || view.state.readOnly) return false;
  view.dispatch({
    changes: revertChange(view.state, hunk),
    annotations: isolateHistory.of("full"),
    userEvent: "input.revert",
    scrollIntoView: true,
  });
  return true;
}

const actions: PeekActions = {
  previous: (view) => step(view, -1),
  next: (view) => step(view, 1),
  revert: (view) => {
    const { peek } = view.state.field(gutterField);
    if (peek !== null) revertHunk(view, peek);
    view.focus();
  },
  close: (view) => {
    openPeek(view, null);
    view.focus();
  },
};

function peekDecorations(editor: EditorState): DecorationSet {
  const state = editor.field(gutterField);
  const hunk = state.peek === null ? undefined : state.hunks[state.peek];
  if (!hunk || state.base === null) return Decoration.none;
  const builder = new RangeSetBuilder<Decoration>();
  const doc = editor.doc;
  const peek = new GitGutterPeek(
    state.peek!,
    state.hunks.length,
    stagedText(state.base, hunk),
    !editor.readOnly,
    actions,
  );
  if (hunk.kind === "deleted" && !hunk.atEnd) {
    builder.add(
      hunk.from,
      hunk.from,
      Decoration.widget({ widget: peek, block: true, side: -1 }),
    );
    return builder.finish();
  }
  if (hunk.kind !== "deleted") {
    const line = Decoration.line({
      class: `git-gutter-peek-line git-gutter-peek-line--${hunk.kind}`,
    });
    for (let position = hunk.from; position <= hunk.to;) {
      const current = doc.lineAt(position);
      builder.add(current.from, current.from, line);
      position = current.to + 1;
    }
  }
  builder.add(
    hunk.to,
    hunk.to,
    Decoration.widget({ widget: peek, block: true, side: 1 }),
  );
  return builder.finish();
}

const peekField = EditorView.decorations.compute(
  [gutterField],
  peekDecorations,
);

const changeGutter = gutter({
  class: "git-gutter",
  lineMarker(view, line) {
    return markerAt(
      view.state.field(gutterField),
      line.from,
      line.to,
      view.state.doc.length,
    );
  },
  lineMarkerChange: (update) =>
    update.startState.field(gutterField) !== update.state.field(gutterField),
  domEventHandlers: {
    mousedown(view, line) {
      const state = view.state.field(gutterField);
      const index = hunkAtLine(
        state.hunks,
        line.from,
        line.to,
        view.state.doc.length,
      );
      if (index === null) return false;
      openPeek(view, state.peek === index ? null : index);
      return true;
    },
  },
});

/**
 * Sends the worker the document once, then only the edits made since the
 * last message, each a pause after typing stops. A keystroke costs a
 * ChangeSet compose and a timer reset, never the text.
 */
type ResultSink = { current: (result: GutterResult) => void };

/**
 * The document as a string the caller already holds (a File's text as
 * loaded), or null. Opening with it skips building a 1 MiB string on the
 * main thread; a wrong length falls back to building it.
 */
export type KnownText = () => string | null;

function documentText(doc: Text, knownText: KnownText) {
  const known = knownText();
  return known !== null && known.length === doc.length ? known : doc.toString();
}

function driver(
  connection: GutterConnection,
  sink: ResultSink,
  knownText: KnownText,
) {
  return ViewPlugin.define((view) => {
    let opened = false;
    let baseChanged = false;
    let pending: ChangeSet | null = null;
    let timer: ReturnType<typeof setTimeout> | null = null;
    let version = 0;
    let posted: Text | null = null;

    const flush = () => {
      timer = null;
      const { base } = view.state.field(gutterField);
      if (base === null) return;
      version += 1;
      if (!opened) {
        connection.post({
          type: "open",
          id: connection.id,
          version,
          base,
          doc: documentText(view.state.doc, knownText),
        });
        opened = true;
      } else {
        if (pending) {
          connection.post({
            type: "edit",
            id: connection.id,
            version,
            changes: pending.toJSON(),
          });
        }
        if (baseChanged) {
          if (pending) version += 1;
          connection.post({ type: "base", id: connection.id, version, base });
        }
      }
      pending = null;
      baseChanged = false;
      posted = view.state.doc;
    };
    const schedule = () => {
      if (timer !== null) clearTimeout(timer);
      timer = setTimeout(flush, GUTTER_IDLE_MS);
    };

    sink.current = (result) => {
      if (result.version !== version || !posted) return;
      let hunks = hunksFromLines(result.hunks, posted);
      // Edits made while the worker ran: carry the result through them.
      if (pending) {
        const changes = pending;
        hunks = hunks.map((hunk) => ({
          ...hunk,
          from: changes.mapPos(hunk.from, hunk.kind === "deleted" ? 1 : -1),
          to: changes.mapPos(hunk.to, 1),
        }));
      }
      view.dispatch({ effects: setGutterHunks.of(hunks) });
    };

    return {
      update(update: ViewUpdate) {
        const before = update.startState.field(gutterField).base;
        const after = update.state.field(gutterField).base;
        if (after === null) {
          if (opened) connection.post({ type: "close", id: connection.id });
          opened = false;
          pending = null;
          return;
        }
        if (update.docChanged && opened) {
          pending = pending ? pending.compose(update.changes) : update.changes;
        }
        if (before !== after) baseChanged = true;
        if (update.docChanged || before !== after) schedule();
      },
      destroy() {
        if (timer !== null) clearTimeout(timer);
      },
    };
  });
}

const peekKeymap = Prec.high(
  keymap.of([
    {
      key: "Escape",
      run: (view) => {
        if (view.state.field(gutterField).peek === null) return false;
        openPeek(view, null);
        return true;
      },
    },
  ]),
);

export interface GitGutter {
  setBase: (base: string | null) => void;
  destroy: () => void;
}

const attached = new WeakMap<EditorView, GitGutter>();

/** Adds change markers to an editor; `setBase` gives them the staged text. */
export function attachGitGutter(
  view: EditorView,
  pool: GutterPool,
  knownText: KnownText = () => null,
): GitGutter {
  attached.get(view)?.destroy();
  const compartment = new Compartment();
  const sink: ResultSink = { current: () => undefined };
  const connection = pool.connect((result) => sink.current(result));
  const extension: Extension = [
    gutterField,
    changeGutter,
    peekField,
    peekKeymap,
    driver(connection, sink, knownText),
  ];
  view.dispatch({
    effects: StateEffect.appendConfig.of(compartment.of(extension)),
  });
  let destroyed = false;
  const gutterHandle: GitGutter = {
    setBase(base) {
      if (destroyed) return;
      if (view.state.field(gutterField, false)?.base === base) return;
      view.dispatch({ effects: setGutterBase.of(base) });
    },
    destroy() {
      if (destroyed) return;
      destroyed = true;
      attached.delete(view);
      connection.close();
      try {
        view.dispatch({ effects: compartment.reconfigure([]) });
      } catch {
        // The editor was destroyed first, taking the gutter with it.
      }
    },
  };
  attached.set(view, gutterHandle);
  return gutterHandle;
}
