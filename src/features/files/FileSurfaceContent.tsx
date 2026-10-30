import { useEffect, type KeyboardEventHandler, type RefObject } from "react";
import { type Text } from "@codemirror/state";
import { LazyModuleBoundary } from "../../ui/LazyModuleBoundary";
import { ResourceStateNotice } from "../../ui/ResourceStateNotice";
import { type ActiveEditorState } from "./fileEditorController";
import { EditorAdapter, type EditorAdapterHandle } from "./EditorAdapter";
import {
  MAX_EDITABLE_CONTENT_BYTES,
  contentText,
  type EditorContent,
  type EditorContentBase,
} from "./editorContent";
import { FileEditorHeader } from "./FileEditorHeader";
import { type FindControl, type FindOpenRequest } from "./FileFindBar";
import { FileMarkdownPreview } from "./FileMarkdownPreview";
import { canPreviewDiagram, canPreviewMarkdown } from "./markdownPreview";
import { AlertTriangle, X } from "../../ui/leanIcons";
import { type EditorConflict } from "../../domain/workbench";
import { MAX_GOTO_LINE_INPUT_CHARS } from "./goToLine";

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

interface FileSurfaceGoToView {
  inputRef: RefObject<HTMLInputElement | null>;
  query: string;
  status: string;
  onQueryChange: (query: string) => void;
  onSubmit: () => void;
  onClose: () => void;
}

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
        <FileGoToLineBar
          inputRef={goTo.inputRef}
          query={goTo.query}
          status={goTo.status}
          onQueryChange={goTo.onQueryChange}
          onSubmit={goTo.onSubmit}
          onClose={goTo.onClose}
        />
      ) : null}
      {previewing ? (
        <FileMarkdownPreview
          source={contentText(editor.content)}
          asDiagram={canPreviewDiagram(path) && !canPreviewMarkdown(path)}
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
          />
        </div>
      )}
    </section>
  );
}

interface FileEditorNoticesProps {
  conflict: EditorConflict | null;
  message: string | null;
}

function FileEditorNotices({ conflict, message }: FileEditorNoticesProps) {
  return (
    <>
      {conflict ? (
        <div className="editor-notice editor-notice--conflict" role="alert">
          <AlertTriangle size={16} aria-hidden="true" />
          <span>
            <strong>External change detected.</strong> {conflict.message}
          </span>
        </div>
      ) : null}
      {message && !conflict ? (
        <div className="editor-notice editor-notice--error" role="alert">
          <AlertTriangle size={16} aria-hidden="true" />
          <span>{message}</span>
        </div>
      ) : null}
    </>
  );
}

interface FileGoToLineBarProps {
  inputRef: RefObject<HTMLInputElement | null>;
  query: string;
  status: string;
  onQueryChange: (query: string) => void;
  onSubmit: () => void;
  onClose: () => void;
}

function FileGoToLineBar({
  inputRef,
  query,
  status,
  onQueryChange,
  onSubmit,
  onClose,
}: FileGoToLineBarProps) {
  return (
    <div className="editor-find" role="search" aria-label="Go to line">
      <label className="editor-find__query">
        <span className="sr-only">Go to line</span>
        <input
          ref={inputRef}
          type="text"
          aria-label="Go to line"
          maxLength={MAX_GOTO_LINE_INPUT_CHARS}
          value={query}
          onChange={(event) =>
            onQueryChange(
              event.target.value.slice(0, MAX_GOTO_LINE_INPUT_CHARS),
            )
          }
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              onSubmit();
            }
          }}
        />
      </label>
      <span className="editor-find__status" role="status">
        {status}
      </span>
      <button
        className="icon-button editor-find__button"
        type="button"
        aria-label="Close go to line"
        onClick={onClose}
      >
        <X size={14} aria-hidden="true" />
      </button>
    </div>
  );
}
