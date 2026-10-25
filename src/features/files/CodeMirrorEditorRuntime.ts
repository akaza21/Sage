import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import {
  HighlightStyle,
  defaultHighlightStyle,
  indentUnit,
  syntaxHighlighting,
  language as languageFacet,
} from "@codemirror/language";
import {
  Compartment,
  EditorState,
  Transaction,
  type Extension,
  type Text,
} from "@codemirror/state";
import {
  EditorView,
  highlightActiveLine,
  highlightActiveLineGutter,
  highlightWhitespace,
  keymap,
  lineNumbers,
} from "@codemirror/view";

import {
  DEFAULT_EDITOR_APPEARANCE,
  sameValues,
  type EditorAppearance,
  type EditorEditing,
} from "./editorAppearance";
import {
  editorLanguageForPath,
  loadEditorLanguage,
  type EditorLanguage,
} from "./editorLanguage";
import { highlightTree } from "@lezer/highlight";
import { StyleModule } from "style-mod";
import { MAX_MARKDOWN_PREVIEW_BLOCK_CHARS } from "./markdownLimits";

interface RuntimeProps {
  ariaLabel: string;
  /**
   * Authoritative controlled document identity.  Callers include the resource,
   * durable revision and content fingerprint so cosmetic updates never need to
   * read the CodeMirror document.
   */
  documentIdentity?: string;
  path: string;
  content: EditorContent;
  disabled: boolean;
  maxContentLength: number;
  appearance?: EditorAppearance;
  /** The editing features to load; none for a read-only view. */
  editing?: EditorEditing | null;
  /** Hands over the editor's immutable document; never a built string. */
  onChange: (content: Text, base: EditorContentBase | null) => void;
  onCaretOffset?: (offset: number, document: Text) => void;
}

interface RuntimeOptions extends RuntimeProps {
  parent: HTMLElement;
}

export interface CodeMirrorEditorRuntime {
  destroy: () => void;
  focus: () => void;
  selection: () => { start: number; end: number };
  setSelection: (start: number, end: number) => void;
  update: (props: RuntimeProps) => void;
}

import type { EditorContent, EditorContentBase } from "./editorContent";
import type * as EditingFeatures from "./editorEditing";
import { utf8LengthField, withinUtf8Limit } from "./utf8Tracking";

/*
 * Document syntax palette resolves from --doc-syntax-* tokens (authority in
 * src/styles/global.css). Specs are the public default highlight table with
 * only color sources replaced, so tag order and non-color treatments match
 * the installed runtime exactly and prior pixels are preserved in both modes.
 * Null entries keep specs without color. A CI test pins count, order, and
 * treatments so dependency drift cannot silently mis-map a token.
 */
const DOCUMENT_SYNTAX_TOKENS_BY_SPEC_INDEX: readonly (string | null)[] = [
  "var(--doc-syntax-meta)",
  null,
  null,
  null,
  null,
  null,
  "var(--doc-syntax-keyword)",
  "var(--doc-syntax-atom)",
  "var(--doc-syntax-literal)",
  "var(--doc-syntax-string)",
  "var(--doc-syntax-regexp)",
  "var(--doc-syntax-variable-definition)",
  "var(--doc-syntax-variable-local)",
  "var(--doc-syntax-type)",
  "var(--doc-syntax-class)",
  "var(--doc-syntax-variable-special)",
  "var(--doc-syntax-property-definition)",
  "var(--doc-syntax-comment)",
  "var(--doc-syntax-invalid)",
];

const documentSyntaxHighlightStyle = HighlightStyle.define(
  defaultHighlightStyle.specs.map((spec, index) => {
    const token = DOCUMENT_SYNTAX_TOKENS_BY_SPEC_INDEX[index];
    if (token == null) {
      return spec;
    }
    const { color: _replacedDefaultColor, ...rest } = spec;
    void _replacedDefaultColor;
    return { ...rest, color: token };
  }),
);

export const MAX_CODE_HIGHLIGHT_TOKENS = 512;
export interface CodeSpan {
  from: number;
  to: number;
  className: string;
}

export async function highlightCodeSource(
  source: string,
  language: EditorLanguage,
  current = () => true,
): Promise<CodeSpan[] | null> {
  if (
    !current() ||
    language === "plain" ||
    source.length > MAX_MARKDOWN_PREVIEW_BLOCK_CHARS
  )
    return null;
  const extensions = await loadEditorLanguage(language);
  if (!current()) return null;
  const parser = EditorState.create({ extensions }).facet(
    languageFacet,
  )?.parser;
  if (!parser) return null;
  const spans: CodeSpan[] = [];
  highlightTree(
    parser.parse(source),
    documentSyntaxHighlightStyle,
    (from, to, className) => {
      if (spans.length < MAX_CODE_HIGHLIGHT_TOKENS)
        spans.push({ from, to, className });
    },
  );
  if (documentSyntaxHighlightStyle.module)
    StyleModule.mount(document, documentSyntaxHighlightStyle.module);
  return spans;
}

interface DocumentBase {
  content: string;
  document: Text;
}

/**
 * The base each emitted document descends from, so an editor remounted on a
 * document from the store (preview toggle, read-only change) still answers
 * "unchanged" from shared structure.
 */
const basesOfEmitted = new WeakMap<Text, DocumentBase>();

function baseFor(content: string, document: Text): DocumentBase | null {
  // CodeMirror reads a lone "\r" as a line break, so its document would not
  // round-trip to `content`; the controller then compares text instead.
  return content.includes("\r") ? null : { content, document };
}

/**
 * `sameContent`, restated here: a value import of `editorContent` would pull
 * it into the CodeMirror chunk and make the File surface load CodeMirror
 * eagerly.
 */
function sameText(left: EditorContent, right: EditorContent) {
  if (left === right) return true;
  if (left.length !== right.length) return false;
  return typeof left !== "string" && typeof right !== "string"
    ? left.eq(right)
    : String(left) === String(right);
}

function appearanceExtensions(appearance: EditorAppearance): Extension {
  return [
    EditorState.tabSize.of(appearance.tabSize),
    indentUnit.of(" ".repeat(appearance.tabSize)),
    appearance.wordWrap ? EditorView.lineWrapping : [],
    appearance.lineNumbers ? [lineNumbers(), highlightActiveLineGutter()] : [],
    appearance.showWhitespace ? highlightWhitespace() : [],
    EditorView.theme({
      ".cm-scroller": { fontSize: `${appearance.fontSize}px` },
    }),
  ];
}

function editableExtensions(props: RuntimeProps): Extension {
  return [
    EditorState.readOnly.of(props.disabled),
    EditorView.editable.of(!props.disabled),
    EditorView.contentAttributes.of({
      "aria-label": props.ariaLabel,
      "aria-multiline": "true",
      "aria-readonly": props.disabled ? "true" : "false",
      "data-max-content-bytes": String(props.maxContentLength),
      spellcheck: "false",
    }),
  ];
}

type EditingModule = typeof EditingFeatures;

// Loaded on demand for the first editable File, then reused synchronously so
// later editors start with every feature in place.
let editingModule: EditingModule | null = null;
let editingLoad: Promise<EditingModule> | null = null;

function editingExtensionsFor(editing: EditorEditing | null | undefined) {
  return editing && editingModule
    ? editingModule.editingExtensions(editing)
    : [];
}

function loadEditing(): Promise<EditingModule> {
  editingLoad ??= import("./editorEditing").then(
    (loaded) => (editingModule = loaded),
    (error: unknown) => {
      editingLoad = null;
      throw error;
    },
  );
  return editingLoad;
}

function sameEditableConfiguration(
  left: RuntimeProps,
  right: RuntimeProps,
): boolean {
  return (
    left.ariaLabel === right.ariaLabel &&
    left.disabled === right.disabled &&
    left.maxContentLength === right.maxContentLength
  );
}

export function createCodeMirrorEditorRuntime(
  options: RuntimeOptions,
): CodeMirrorEditorRuntime {
  const editableCompartment = new Compartment();
  const languageCompartment = new Compartment();
  const appearanceCompartment = new Compartment();
  const editingCompartment = new Compartment();
  let current: RuntimeProps = {
    ...options,
    appearance: options.appearance ?? DEFAULT_EDITOR_APPEARANCE,
  };
  let applyingControlledContent = false;
  // A local transaction already serializes its exact content through onChange.
  // Its store echo must not become a controlled replacement (which clears undo).
  let lastLocalContent: EditorContent = options.content;
  // The last string the editor was given, and the document built from it.
  // Edits share its unchanged structure, so `eq` against it costs the edit.
  let base: DocumentBase | null = null;
  let languageGeneration = 0;
  let destroyed = false;
  const view = new EditorView({
    parent: options.parent,
    state: EditorState.create({
      doc: options.content,
      extensions: [
        highlightActiveLine(),
        history(),
        // Ahead of the base keymap, so the editing chords take precedence.
        editingCompartment.of(editingExtensionsFor(current.editing)),
        keymap.of([...historyKeymap, ...defaultKeymap]),
        syntaxHighlighting(documentSyntaxHighlightStyle, { fallback: true }),
        editableCompartment.of(editableExtensions(current)),
        languageCompartment.of([]),
        appearanceCompartment.of(
          appearanceExtensions(current.appearance ?? DEFAULT_EDITOR_APPEARANCE),
        ),
        utf8LengthField,
        // Costs the size of the edit, not the document (EP-01).
        EditorState.transactionFilter.of((transaction) =>
          withinUtf8Limit(transaction, current.maxContentLength)
            ? transaction
            : [],
        ),
        EditorView.updateListener.of((update) => {
          if (update.docChanged && !applyingControlledContent) {
            const document = update.state.doc;
            lastLocalContent = document;
            if (base) basesOfEmitted.set(document, base);
            current.onChange(
              document,
              base && {
                content: base.content,
                unchanged: document.eq(base.document),
              },
            );
          }
          if (
            current.onCaretOffset &&
            (update.selectionSet || update.docChanged)
          ) {
            current.onCaretOffset(
              update.state.selection.main.head,
              update.state.doc,
            );
          }
        }),
      ],
    }),
  });

  function selectLanguage(path: string) {
    const generation = ++languageGeneration;
    view.dispatch({ effects: languageCompartment.reconfigure([]) });
    void loadEditorLanguage(editorLanguageForPath(path))
      .then((extension) => {
        if (!destroyed && generation === languageGeneration) {
          view.dispatch({
            effects: languageCompartment.reconfigure(extension),
          });
        }
      })
      .catch(() => {
        if (!destroyed && generation === languageGeneration) {
          view.dispatch({ effects: languageCompartment.reconfigure([]) });
        }
      });
  }

  // Applies the editing features `current` asks for, loading them first when
  // needed. The editor works without them while they load (or if they fail).
  function applyEditing() {
    if (current.editing && !editingModule) {
      void loadEditing()
        .then(() => {
          if (!destroyed && current.editing) applyEditing();
        })
        .catch(() => undefined);
      return;
    }
    view.dispatch({
      effects: editingCompartment.reconfigure(
        editingExtensionsFor(current.editing),
      ),
    });
  }

  if (current.editing && !editingModule) applyEditing();
  selectLanguage(options.path);
  base =
    typeof options.content === "string"
      ? baseFor(options.content, view.state.doc)
      : (basesOfEmitted.get(options.content) ?? null);
  current.onCaretOffset?.(view.state.selection.main.head, view.state.doc);

  return {
    destroy: () => {
      destroyed = true;
      languageGeneration += 1;
      view.destroy();
    },
    focus: () => view.focus(),
    selection: () => ({
      start: view.state.selection.main.from,
      end: view.state.selection.main.to,
    }),
    setSelection: (start, end) => {
      const anchor = Math.max(0, Math.min(start, view.state.doc.length));
      const head = Math.max(anchor, Math.min(end, view.state.doc.length));
      view.dispatch({
        selection: { anchor, head },
        scrollIntoView: true,
      });
    },
    update: (props) => {
      const previous = current;
      const next = {
        ...props,
        appearance: props.appearance ?? DEFAULT_EDITOR_APPEARANCE,
      };
      current = next;
      const effects = [];
      if (!sameEditableConfiguration(previous, next)) {
        effects.push(editableCompartment.reconfigure(editableExtensions(next)));
      }
      if (
        !sameValues(
          previous.appearance ?? DEFAULT_EDITOR_APPEARANCE,
          next.appearance ?? DEFAULT_EDITOR_APPEARANCE,
        )
      ) {
        effects.push(
          appearanceCompartment.reconfigure(
            appearanceExtensions(next.appearance ?? DEFAULT_EDITOR_APPEARANCE),
          ),
        );
      }
      if (effects.length > 0) {
        view.dispatch({ effects });
      }
      if (!sameValues(previous.editing, next.editing)) {
        applyEditing();
      }
      // Do not stringify the editor document for unrelated prop updates.  A
      // changed identity is deliberately stronger than a local revision: it
      // includes the resource and controlled content, so reload/save/undo
      // reconciliation and same-text resource changes remain authoritative.
      const identityChanged =
        next.documentIdentity !== undefined &&
        previous.documentIdentity !== undefined
          ? next.documentIdentity !== previous.documentIdentity
          : !sameText(view.state.doc, props.content);
      // Identity normally avoids touching content, but it cannot mask an
      // authoritative content disagreement from an external reload.
      const controlledDocumentChanged =
        identityChanged || props.content !== previous.content;
      if (
        controlledDocumentChanged &&
        !sameText(props.content, lastLocalContent)
      ) {
        applyingControlledContent = true;
        try {
          view.dispatch({
            changes: {
              from: 0,
              to: view.state.doc.length,
              insert: props.content,
            },
            annotations: Transaction.addToHistory.of(false),
          });
        } finally {
          applyingControlledContent = false;
        }
        lastLocalContent = props.content;
      }
      if (controlledDocumentChanged && typeof props.content === "string") {
        base = baseFor(props.content, view.state.doc);
      }
      if (props.path !== previous.path) {
        selectLanguage(props.path);
      }
    },
  };
}
