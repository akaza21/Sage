import type { KeyboardEventHandler, RefObject } from "react";
import type { Text } from "@codemirror/state";

import { ResourceStateNotice } from "../../ui/ResourceStateNotice";
import type { ActiveEditorState } from "./fileEditorController";
import { EditorAdapter, type EditorAdapterHandle } from "./EditorAdapter";
import {
  contentText,
  type EditorContent,
  type EditorContentBase,
} from "./editorContent";
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
            readOnly={readOnly || reviewOnly}
            onChange={onEditorChange}
            onCaretOffset={onCaretOffset}
          />
        </div>
      )}
    </section>
  );
}
