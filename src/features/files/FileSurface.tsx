import { AlertTriangle, FileCode2, Save, Undo2 } from "lucide-react";
import { useEffect } from "react";

import { useWorkbenchStore } from "../../app/store";
import { sameResource } from "../../domain/resourceIdentity";
import type { WorkbenchTab } from "../../domain/workbench";

const SNAPSHOT_DELAY_MS = 750;

export function FileSurface({ tab }: { tab: WorkbenchTab }) {
  const resource = tab.resource;
  const path = resource?.relativePath ?? tab.title;
  const editor = useWorkbenchStore((state) => state.activeEditor);
  const loadEditor = useWorkbenchStore((state) => state.loadEditor);
  const updateEditorContent = useWorkbenchStore(
    (state) => state.updateEditorContent,
  );
  const flushActiveEditor = useWorkbenchStore(
    (state) => state.flushActiveEditor,
  );
  const saveActiveEditor = useWorkbenchStore((state) => state.saveActiveEditor);
  const discardActiveEditor = useWorkbenchStore(
    (state) => state.discardActiveEditor,
  );
  const revision = editor?.phase === "ready" ? editor.revision : 0;
  const persistedRevision =
    editor?.phase === "ready" ? editor.persistedRevision : 0;

  useEffect(() => {
    if (resource) {
      void loadEditor(resource);
    }
  }, [loadEditor, resource]);

  useEffect(() => {
    if (
      editor?.phase !== "ready" ||
      !sameResource(editor.resource, resource) ||
      revision <= persistedRevision
    ) {
      return;
    }
    const timeout = window.setTimeout(() => {
      void flushActiveEditor().catch(() => undefined);
    }, SNAPSHOT_DELAY_MS);
    return () => window.clearTimeout(timeout);
  }, [
    editor?.resource,
    editor?.phase,
    flushActiveEditor,
    path,
    persistedRevision,
    resource,
    revision,
  ]);

  if (
    !resource ||
    !editor ||
    !sameResource(editor.resource, resource) ||
    editor.phase === "loading"
  ) {
    return (
      <section className="resource-state" aria-live="polite">
        Loading {path}…
      </section>
    );
  }
  if (editor.phase === "error") {
    return (
      <section className="resource-state resource-state--error">
        <strong>Could not open {path}</strong>
        <span>{editor.message}</span>
      </section>
    );
  }

  const conflict = editor.document.conflict;
  return (
    <section className="file-surface" aria-label={`File editor ${path}`}>
      <header className="file-header">
        <span className="file-header__path">
          <FileCode2 size={16} aria-hidden="true" />
          {path}
        </span>
        <span className="file-header__metadata">
          UTF-8 · {editor.document.lineEnding.toUpperCase()} ·{" "}
          {editor.document.disk.byteLen.toLocaleString()} disk bytes
          {editor.persisting ? " · Recovering…" : ""}
        </span>
        <span className="file-header__actions">
          <button
            className="button button--compact"
            type="button"
            disabled={!tab.dirty || editor.saving}
            onClick={() => void discardActiveEditor()}
          >
            <Undo2 size={14} aria-hidden="true" />
            Discard
          </button>
          <button
            className="button button--compact button--primary"
            type="button"
            disabled={!tab.dirty || editor.saving || editor.persisting}
            onClick={() => void saveActiveEditor()}
          >
            <Save size={14} aria-hidden="true" />
            {editor.saving ? "Saving…" : "Save"}
          </button>
        </span>
      </header>
      {conflict ? (
        <div className="editor-notice editor-notice--conflict" role="alert">
          <AlertTriangle size={16} aria-hidden="true" />
          <span>
            <strong>External change detected.</strong> {conflict.message}
          </span>
        </div>
      ) : null}
      {editor.message ? (
        <div className="editor-notice editor-notice--error" role="alert">
          <AlertTriangle size={16} aria-hidden="true" />
          <span>{editor.message}</span>
        </div>
      ) : null}
      <textarea
        className="editor-input"
        aria-label={`Edit ${path}`}
        value={editor.content}
        disabled={editor.saving}
        spellCheck={false}
        onChange={(event) => updateEditorContent(event.target.value)}
        onKeyDown={(event) => {
          if ((event.ctrlKey || event.metaKey) && event.key === "s") {
            event.preventDefault();
            void saveActiveEditor();
          }
        }}
      />
    </section>
  );
}
