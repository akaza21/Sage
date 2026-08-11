import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import {
  defaultHighlightStyle,
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
  keymap,
  lineNumbers,
} from "@codemirror/view";

import { editorLanguageForPath, loadEditorLanguage } from "./editorLanguage";

interface RuntimeProps {
  ariaLabel: string;
  path: string;
  content: string;
  disabled: boolean;
  maxContentLength: number;
  onChange: (content: string) => void;
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

const textEncoder = new TextEncoder();

function withinUtf8Limit(document: Text, limit: number) {
  if (document.length > limit) {
    return false;
  }
  if (document.length <= Math.floor(limit / 3)) {
    return true;
  }
  return textEncoder.encode(document.toString()).byteLength <= limit;
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

export function createCodeMirrorEditorRuntime(
  options: RuntimeOptions,
): CodeMirrorEditorRuntime {
  const editableCompartment = new Compartment();
  const languageCompartment = new Compartment();
  let current: RuntimeProps = options;
  let applyingControlledContent = false;
  let languageGeneration = 0;
  let destroyed = false;
  const view = new EditorView({
    parent: options.parent,
    state: EditorState.create({
      doc: options.content,
      extensions: [
        lineNumbers(),
        highlightActiveLineGutter(),
        highlightActiveLine(),
        history(),
        keymap.of([...historyKeymap, ...defaultKeymap]),
        syntaxHighlighting(defaultHighlightStyle, { fallback: true }),
        editableCompartment.of(editableExtensions(options)),
        languageCompartment.of([]),
        EditorState.transactionFilter.of((transaction) =>
          !transaction.docChanged ||
          withinUtf8Limit(transaction.newDoc, current.maxContentLength)
            ? transaction
            : [],
        ),
        EditorView.updateListener.of((update) => {
          if (update.docChanged && !applyingControlledContent) {
            current.onChange(update.state.doc.toString());
          }
        }),
      ],
    }),
  });

  function selectLanguage(path: string) {
    const generation = ++languageGeneration;
    void loadEditorLanguage(editorLanguageForPath(path)).then((extension) => {
      if (!destroyed && generation === languageGeneration) {
        view.dispatch({ effects: languageCompartment.reconfigure(extension) });
      }
    });
  }

  selectLanguage(options.path);

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
      const previousPath = current.path;
      current = props;
      view.dispatch({
        effects: editableCompartment.reconfigure(editableExtensions(props)),
      });
      if (view.state.doc.toString() !== props.content) {
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
      }
      if (props.path !== previousPath) {
        selectLanguage(props.path);
      }
    },
  };
}
