import {
  forwardRef,
  useImperativeHandle,
  useLayoutEffect,
  useRef,
  useState,
} from "react";

import type { CodeMirrorEditorRuntime } from "./CodeMirrorEditorRuntime";

export interface EditorSelection {
  start: number;
  end: number;
}

export interface EditorAdapterHandle {
  focus: () => void;
  selection: () => EditorSelection;
  setSelection: (start: number, end: number) => void;
}

interface EditorAdapterProps {
  ariaLabel: string;
  path: string;
  content: string;
  disabled: boolean;
  maxContentLength: number;
  onChange: (content: string) => void;
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
        const runtime = createCodeMirrorEditorRuntime({
          parent,
          ...latestPropsRef.current,
        });
        runtimeRef.current = runtime;
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
      runtimeRef.current?.destroy();
      runtimeRef.current = null;
    };
  }, []);

  useLayoutEffect(() => {
    const runtime = runtimeRef.current;
    if (runtime) {
      runtime.update(latestPropsRef.current);
    }
  }, [
    props.ariaLabel,
    props.content,
    props.disabled,
    props.maxContentLength,
    props.onChange,
    props.path,
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
