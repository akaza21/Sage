import type { KeyboardEventHandler, RefObject } from "react";

import { ResourceStateNotice } from "../../ui/ResourceStateNotice";
import type { ActiveEditorState } from "./fileEditorController";
import { EditorAdapter, type EditorAdapterHandle } from "./EditorAdapter";
import { FileEditorHeader } from "./FileEditorHeader";
import { FileEditorNotices } from "./FileEditorNotices";
import { FileFindBar } from "./FileFindBar";
import { FileGoToLineBar } from "./FileGoToLineBar";
import { FileMarkdownPreview } from "./FileMarkdownPreview";
import { MAX_EDITABLE_CONTENT_BYTES } from "./inFileFind";
import { canPreviewDiagram, canPreviewMarkdown } from "./markdownPreview";

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
  inputRef: RefObject<HTMLInputElement | null>;
  query: string;
  status: string;
  caseSensitive: boolean;
  hasMatches: boolean;
  onQueryChange: (query: string) => void;
  onMove: (direction: 1 | -1) => void;
  onToggleCase: () => void;
  onClose: () => void;
}

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
  previewing,
  transitioning,
  readOnly = false,
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
  previewing: boolean;
  transitioning: boolean;
  readOnly?: boolean;
  onCaretOffset: (offset: number) => void;
  onEditorChange: (content: string) => void;
  onSurfaceKeyDown: KeyboardEventHandler<HTMLElement>;
  onTogglePreview: () => void;
}) {
  return (
    <section
      className="file-surface resource-document"
      aria-label={`File editor ${path}`}
      aria-keyshortcuts="F12"
      onKeyDownCapture={onSurfaceKeyDown}
    >
      <FileEditorHeader
        path={path}
        lineEnding={editor.document.lineEnding}
        diskByteLen={editor.document.disk.byteLen}
        persisting={editor.persisting}
        dirty={dirty}
        saving={editor.saving}
        transitioning={transitioning}
        canPreview={canPreviewMarkdown(path) || canPreviewDiagram(path)}
        previewing={previewing}
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
          source={editor.content}
          asDiagram={canPreviewDiagram(path) && !canPreviewMarkdown(path)}
        />
      ) : (
        <div className="file-editor-stage">
          {find ? (
            <FileFindBar
              inputRef={find.inputRef}
              query={find.query}
              status={find.status}
              caseSensitive={find.caseSensitive}
              hasMatches={find.hasMatches}
              onQueryChange={find.onQueryChange}
              onMove={find.onMove}
              onToggleCase={find.onToggleCase}
              onClose={find.onClose}
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
            onChange={onEditorChange}
            onCaretOffset={onCaretOffset}
          />
        </div>
      )}
    </section>
  );
}
