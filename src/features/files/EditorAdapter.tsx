import type { Text } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";
import {
  forwardRef,
  useImperativeHandle,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { useShallow } from "zustand/react/shallow";

import {
  editorAppearanceFrom,
  editorEditingFrom,
  useEditorSettingsStore,
} from "../../app/editorSettingsStore";
import type { CodeMirrorEditorRuntime } from "./CodeMirrorEditorRuntime";
import type { EditorContent, EditorContentBase } from "./editorContent";

export interface EditorSelection {
  start: number;
  end: number;
}

export interface EditorAdapterHandle {
  focus: () => void;
  selection: () => EditorSelection;
  setSelection: (start: number, end: number) => void;
  view: () => EditorView | null;
}

interface EditorAdapterProps {
  ariaLabel: string;
  documentIdentity?: string;
  path: string;
  content: EditorContent;
  disabled: boolean;
  readOnly?: boolean;
  maxContentLength: number;
  onChange: (content: Text, base: EditorContentBase | null) => void;
  onCaretOffset?: (offset: number, document: Text) => void;
  onLimitExceeded?: () => void;
  /** The mounted view, then null on teardown; `text` is the initial source string, not live text. */
  onView?: (view: EditorView | null, text: string | null) => void;
}

export const EditorAdapter = forwardRef<
  EditorAdapterHandle,
  EditorAdapterProps
>(function EditorAdapter(props, ref) {
  const mountRef = useRef<HTMLDivElement>(null);
  const runtimeRef = useRef<CodeMirrorEditorRuntime | null>(null);
  const latestPropsRef = useRef(props);
  const pendingFocusRef = useRef(false);
  const pendingSelectionRef = useRef<EditorSelection | null>(null);
  const [loadState, setLoadState] = useState<"loading" | "ready" | "error">(
    "loading",
  );
  const fontSize = useEditorSettingsStore(
    (state) => state.settings["editor.fontSize"],
  );
  const tabSize = useEditorSettingsStore(
    (state) => state.settings["editor.tabSize"],
  );
  const wordWrap = useEditorSettingsStore(
    (state) => state.settings["editor.wordWrap"],
  );
  const lineNumbers = useEditorSettingsStore(
    (state) => state.settings["editor.lineNumbers"],
  );
  const showWhitespace = useEditorSettingsStore(
    (state) => state.settings["editor.renderWhitespace"],
  );
  // One shallow-compared object, so a change to another setting skips it.
  const editing = useEditorSettingsStore(
    useShallow((state) => editorEditingFrom(state.settings)),
  );
  latestPropsRef.current = props;

  useLayoutEffect(() => {
    const parent = mountRef.current;
    if (!parent) {
      return;
    }
    let active = true;
    void import("./CodeMirrorEditorRuntime")
      .then(({ createCodeMirrorEditorRuntime }) => {
        if (!active) {
          return;
        }
        const { settings } = useEditorSettingsStore.getState();
        const runtime = createCodeMirrorEditorRuntime({
          parent,
          ...latestPropsRef.current,
          appearance: editorAppearanceFrom(settings),
          editing: latestPropsRef.current.readOnly
            ? null
            : editorEditingFrom(settings),
        });
        runtimeRef.current = runtime;
        const { content, onView } = latestPropsRef.current;
        onView?.(runtime.view, typeof content === "string" ? content : null);
        const selection = pendingSelectionRef.current;
        if (selection) {
          runtime.setSelection(selection.start, selection.end);
        }
        if (pendingFocusRef.current) {
          runtime.focus();
        }
        setLoadState("ready");
      })
      .catch(() => {
        if (active) {
          setLoadState("error");
        }
      });
    return () => {
      active = false;
      if (runtimeRef.current) latestPropsRef.current.onView?.(null, null);
      runtimeRef.current?.destroy();
      runtimeRef.current = null;
    };
  }, []);

  useLayoutEffect(() => {
    const runtime = runtimeRef.current;
    if (runtime) {
      runtime.update({
        ...latestPropsRef.current,
        appearance: {
          fontSize,
          tabSize,
          wordWrap,
          lineNumbers,
          showWhitespace,
        },
        editing: props.readOnly ? null : editing,
      });
    }
  }, [
    editing,
    props.readOnly,
    fontSize,
    lineNumbers,
    props.ariaLabel,
    props.content,
    props.documentIdentity,
    props.disabled,
    props.maxContentLength,
    props.onChange,
    props.onCaretOffset,
    props.onLimitExceeded,
    props.path,
    tabSize,
    showWhitespace,
    wordWrap,
  ]);

  useImperativeHandle(
    ref,
    () => ({
      focus: () => {
        pendingFocusRef.current = true;
        runtimeRef.current?.focus();
      },
      selection: () =>
        runtimeRef.current?.selection() ??
        pendingSelectionRef.current ?? { start: 0, end: 0 },
      setSelection: (start, end) => {
        pendingSelectionRef.current = { start, end };
        runtimeRef.current?.setSelection(start, end);
      },
      view: () => runtimeRef.current?.view ?? null,
    }),
    [],
  );

  return (
    <div className="editor-input">
      <div ref={mountRef} className="editor-input__mount" />
      {loadState === "loading" ? (
        <div className="editor-loading" role="status">
          Loading editor…
        </div>
      ) : null}
      {loadState === "error" ? (
        <div className="editor-loading editor-loading--error" role="alert">
          Could not load the editor.
        </div>
      ) : null}
    </div>
  );
});
