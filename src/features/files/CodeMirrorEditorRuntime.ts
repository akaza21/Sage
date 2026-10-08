import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import {
  HighlightStyle,
  defaultHighlightStyle,
  indentUnit,
  syntaxHighlighting,
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
  type EditorAppearance,
} from "./editorAppearance";
import { editorLanguageForPath, loadEditorLanguage } from "./editorLanguage";

interface RuntimeProps {
  ariaLabel: string;
  /**
   * Authoritative controlled document identity.  Callers include the resource,
   * durable revision and content fingerprint so cosmetic updates never need to
   * read the CodeMirror document.
   */
  documentIdentity?: string;
  path: string;
  content: string;
  disabled: boolean;
  maxContentLength: number;
  appearance?: EditorAppearance;
  onChange: (content: string) => void;
  onCaretOffset?: (offset: number) => void;
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

import { utf8ByteLength } from "../../domain/text";

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

function withinUtf8Limit(document: Text, limit: number) {
  if (document.length > limit) {
    return false;
  }
  if (document.length <= Math.floor(limit / 3)) {
    return true;
  }
  return utf8ByteLength(document.toString()) <= limit;
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

function sameAppearance(
  left: EditorAppearance,
  right: EditorAppearance,
): boolean {
  return (
    left.fontSize === right.fontSize &&
    left.tabSize === right.tabSize &&
    left.wordWrap === right.wordWrap &&
    left.lineNumbers === right.lineNumbers &&
    left.showWhitespace === right.showWhitespace
  );
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
  let current: RuntimeProps = {
    ...options,
    appearance: options.appearance ?? DEFAULT_EDITOR_APPEARANCE,
  };
  let applyingControlledContent = false;
  // A local transaction already serializes its exact content through onChange.
  // Its store echo must not become a controlled replacement (which clears undo).
  let lastLocalContent = options.content;
  let languageGeneration = 0;
  let destroyed = false;
  const view = new EditorView({
    parent: options.parent,
    state: EditorState.create({
      doc: options.content,
      extensions: [
        highlightActiveLine(),
        history(),
        keymap.of([...historyKeymap, ...defaultKeymap]),
        syntaxHighlighting(documentSyntaxHighlightStyle, { fallback: true }),
        editableCompartment.of(editableExtensions(current)),
        languageCompartment.of([]),
        appearanceCompartment.of(
          appearanceExtensions(current.appearance ?? DEFAULT_EDITOR_APPEARANCE),
        ),
        EditorState.transactionFilter.of((transaction) =>
          !transaction.docChanged ||
          withinUtf8Limit(transaction.newDoc, current.maxContentLength)
            ? transaction
            : [],
        ),
        EditorView.updateListener.of((update) => {
          if (update.docChanged && !applyingControlledContent) {
            lastLocalContent = update.state.doc.toString();
            current.onChange(lastLocalContent);
          }
          if (
            current.onCaretOffset &&
            (update.selectionSet || update.docChanged)
          ) {
            current.onCaretOffset(update.state.selection.main.head);
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

  selectLanguage(options.path);
  current.onCaretOffset?.(view.state.selection.main.head);

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
        !sameAppearance(
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
      // Do not stringify the editor document for unrelated prop updates.  A
      // changed identity is deliberately stronger than a local revision: it
      // includes the resource and controlled content, so reload/save/undo
      // reconciliation and same-text resource changes remain authoritative.
      const identityChanged =
        next.documentIdentity !== undefined &&
        previous.documentIdentity !== undefined
          ? next.documentIdentity !== previous.documentIdentity
          : view.state.doc.toString() !== props.content;
      // Identity normally avoids touching content, but it cannot mask an
      // authoritative content disagreement from an external reload.
      const controlledDocumentChanged =
        identityChanged || props.content !== previous.content;
      if (controlledDocumentChanged && props.content !== lastLocalContent) {
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
      if (props.path !== previous.path) {
        selectLanguage(props.path);
      }
    },
  };
}
