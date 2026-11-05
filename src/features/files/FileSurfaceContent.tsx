import { type Text } from "@codemirror/state";
import { useEffect, type KeyboardEventHandler, type RefObject } from "react";
import type { EditorView } from "@codemirror/view";

import { LazyModuleBoundary } from "../../ui/LazyModuleBoundary";
import { ResourceStateNotice } from "../../ui/ResourceStateNotice";
import { EditorAdapter, type EditorAdapterHandle } from "./EditorAdapter";
import {
  MAX_EDITABLE_CONTENT_BYTES,
  contentText,
  type EditorContent,
  type EditorContentBase,
} from "./editorContent";
import { type ActiveEditorState } from "./fileEditorController";
import { FileEditorHeader } from "./FileEditorHeader";
import { FileEditorNotices } from "./FileEditorNotices";
import { type FindControl, type FindOpenRequest } from "./FileFindBar";
import { FileGoToLine, type FileGoToLineProps } from "./FileGoToLine";
import { canPreviewDiagram, canPreviewMarkdown } from "./filePreviewKind";

const loadMarkdownPreview = () =>
  import("./FileMarkdownPreview").then(({ FileMarkdownPreview }) => ({
    default: FileMarkdownPreview,
  }));

type ReadyEditorState = Extract<ActiveEditorState, { phase: "ready" }>;

export function FileSurfaceResourceState({
  error,
  path,
}: {
  error?: string;
  path: string;
}) {
  return (
    <section
      className="file-surface resource-document"
      aria-label={`File editor ${path}`}
    >
      {error === undefined ? (
        <ResourceStateNotice state="loading" title={`Loading ${path}…`} />
      ) : (
        <ResourceStateNotice
          state="error"
          title={`Could not open ${path}`}
          detail={error}
        />
      )}
    </section>
  );
}

interface FileSurfaceFindView {
  request: FindOpenRequest;
  inputRef: RefObject<HTMLInputElement | null>;
  controlRef: RefObject<FindControl | null>;
  content: EditorContent;
  onClose: () => void;
}

// The Find bar, its matching and its worker load apart from the editor (EP-04).
const loadFindBar = () =>
  import("./FileFindBar").then((module) => ({ default: module.FileFindBar }));

type FileSurfaceGoToView = Omit<FileGoToLineProps, "content">;

export function FileSurfaceContent({
  activeResourceKey,
  dirty,
  editor,
  editorInputRef,
  find,
  goTo,
  languageNavigationStatus,
  path,
  projectName,
  previewing,
  transitioning,
  readOnly = false,
  reviewOnly = false,
  agentMarkdown = false,
  onCaretOffset,
  onLimitExceeded,
  onEditorView,
  onEditorChange,
  onSurfaceKeyDown,
  onTogglePreview,
}: {
  activeResourceKey: string;
  dirty: boolean;
  editor: ReadyEditorState;
  editorInputRef: RefObject<EditorAdapterHandle | null>;
  find: FileSurfaceFindView | null;
  goTo: FileSurfaceGoToView | null;
  languageNavigationStatus: string;
  path: string;
  projectName?: string;
  previewing: boolean;
  transitioning: boolean;
  readOnly?: boolean;
  /** The host shows this File for review only, even during a transition. */
  reviewOnly?: boolean;
  agentMarkdown?: boolean;
  onCaretOffset: (offset: number, document: Text) => void;
  onLimitExceeded: () => void;
  onEditorView?: (view: EditorView | null, text: string | null) => void;
  onEditorChange: (
    content: EditorContent,
    base?: EditorContentBase | null,
  ) => void;
  onSurfaceKeyDown: KeyboardEventHandler<HTMLElement>;
  onTogglePreview: () => void;
}) {
  // Loads the Find bar while idle, so the first Ctrl+F types straight into it.
  useEffect(() => {
    const timer = setTimeout(() => void loadFindBar(), 1000);
    return () => clearTimeout(timer);
  }, []);
  return (
    <section
      className="file-surface resource-document"
      aria-label={`File editor ${path}`}
      aria-keyshortcuts="F12"
      onKeyDownCapture={onSurfaceKeyDown}
    >
      <FileEditorHeader
        path={path}
        projectName={projectName}
        lineEnding={editor.document.lineEnding}
        diskByteLen={editor.document.disk.byteLen}
        persisting={editor.persisting}
        dirty={dirty}
        saving={editor.saving}
        transitioning={transitioning}
        canPreview={canPreviewMarkdown(path) || canPreviewDiagram(path)}
        previewing={previewing}
        agentMarkdown={agentMarkdown}
        onTogglePreview={onTogglePreview}
      />
      <FileEditorNotices
        conflict={editor.document.conflict}
        message={editor.message}
      />
      {languageNavigationStatus ? (
        <p role="status" className="file-language-navigation-status">
          {languageNavigationStatus}
        </p>
      ) : null}
      {goTo && !previewing ? (
        <FileGoToLine {...goTo} content={editor.content} />
      ) : null}
      {previewing ? (
        <LazyModuleBoundary
          load={loadMarkdownPreview}
          label="Markdown preview"
          render={(FileMarkdownPreview) => (
            <FileMarkdownPreview
              source={contentText(editor.content)}
              asDiagram={canPreviewDiagram(path) && !canPreviewMarkdown(path)}
            />
          )}
        />
      ) : (
        <div className="file-editor-stage">
          {find ? (
            <LazyModuleBoundary
              load={loadFindBar}
              label="Find"
              fallback={null}
              render={(FileFindBar) => (
                <FileFindBar
                  {...find}
                  editorRef={editorInputRef}
                  canReplace={
                    !readOnly && !reviewOnly && !transitioning && !editor.saving
                  }
                />
              )}
            />
          ) : null}
          <EditorAdapter
            key={`${activeResourceKey}:${readOnly ? "read-only" : "editable"}`}
            ref={editorInputRef}
            ariaLabel={`Edit ${path}`}
            documentIdentity={`${activeResourceKey}\0${editor.revision}\0${editor.document.buffer.fingerprint}`}
            path={path}
            content={editor.content}
            maxContentLength={MAX_EDITABLE_CONTENT_BYTES}
            disabled={editor.saving || transitioning || readOnly}
            readOnly={readOnly || reviewOnly}
            onChange={onEditorChange}
            onCaretOffset={onCaretOffset}
            onLimitExceeded={onLimitExceeded}
            onView={onEditorView}
          />
        </div>
      )}
    </section>
  );
}
