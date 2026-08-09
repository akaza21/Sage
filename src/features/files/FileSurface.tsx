import {
  AlertTriangle,
  CaseSensitive,
  ChevronDown,
  ChevronUp,
  FileCode2,
  Save,
  Undo2,
  X,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

import { useWorkbenchStore } from "../../app/store";
import { resourceKey, sameResource } from "../../domain/resourceIdentity";
import type { WorkbenchTab } from "../../domain/workbench";
import {
  findLiteralMatches,
  MAX_EDITABLE_CONTENT_BYTES,
  MAX_FIND_QUERY_LENGTH,
} from "./inFileFind";

const SNAPSHOT_DELAY_MS = 750;

export function FileSurface({ tab }: { tab: WorkbenchTab }) {
  const resource = tab.resource;
  const path = resource?.relativePath ?? tab.title;
  const activeResourceKey = resource ? resourceKey(resource) : "";
  const editorInputRef = useRef<HTMLTextAreaElement>(null);
  const findInputRef = useRef<HTMLInputElement>(null);
  const selectedMatchRef = useRef<{ start: number; end: number } | null>(null);
  const [findOpen, setFindOpen] = useState(false);
  const [findQuery, setFindQuery] = useState("");
  const [caseSensitive, setCaseSensitive] = useState(false);
  const [matchCursor, setMatchCursor] = useState({
    contextKey: "",
    index: 0,
  });
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
  const findContextKey = `${activeResourceKey}\0${revision}`;
  const findContent =
    editor?.phase === "ready" && sameResource(editor.resource, resource)
      ? editor.content
      : "";
  const findMatches = useMemo(
    () =>
      findOpen
        ? findLiteralMatches(findContent, findQuery, caseSensitive)
        : { offsets: [], truncated: false, contentLimitExceeded: false },
    [caseSensitive, findContent, findOpen, findQuery],
  );
  const resolvedMatchIndex =
    findMatches.offsets.length === 0
      ? 0
      : matchCursor.contextKey === findContextKey
        ? Math.min(matchCursor.index, findMatches.offsets.length - 1)
        : 0;

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

  useEffect(() => {
    if (findOpen) {
      findInputRef.current?.focus();
      findInputRef.current?.select();
    }
  }, [findOpen]);

  useEffect(() => {
    if (!findOpen) {
      return;
    }
    const input = editorInputRef.current;
    const offset = findMatches.offsets[resolvedMatchIndex];
    if (!input) {
      return;
    }
    if (offset === undefined) {
      const selected = selectedMatchRef.current;
      if (
        selected &&
        input.selectionStart === selected.start &&
        input.selectionEnd === selected.end
      ) {
        input.setSelectionRange(selected.start, selected.start);
      }
      selectedMatchRef.current = null;
      return;
    }
    const start = Math.min(offset, findContent.length);
    const end = Math.min(offset + findQuery.length, findContent.length);
    input.setSelectionRange(start, end);
    selectedMatchRef.current = { start, end };
  }, [
    findContent,
    findMatches.offsets,
    findOpen,
    findQuery.length,
    resolvedMatchIndex,
  ]);

  function openFind() {
    if (findOpen) {
      findInputRef.current?.focus();
      findInputRef.current?.select();
      return;
    }
    setFindOpen(true);
  }

  function closeFind() {
    setFindOpen(false);
    setMatchCursor({ contextKey: findContextKey, index: 0 });
    selectedMatchRef.current = null;
    editorInputRef.current?.focus();
  }

  function moveMatch(direction: 1 | -1) {
    const count = findMatches.offsets.length;
    if (count === 0) {
      return;
    }
    setMatchCursor({
      contextKey: findContextKey,
      index: (resolvedMatchIndex + direction + count) % count,
    });
  }

  const findStatus =
    findQuery.length === 0
      ? "No query"
      : findMatches.contentLimitExceeded
        ? "File exceeds the 1 MiB Find limit"
        : findMatches.offsets.length === 0
          ? "No matches"
          : `${resolvedMatchIndex + 1} of ${findMatches.offsets.length}${findMatches.truncated ? "+" : ""} ${findMatches.offsets.length === 1 && !findMatches.truncated ? "match" : "matches"}`;

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
    <section
      className="file-surface"
      aria-label={`File editor ${path}`}
      onKeyDown={(event) => {
        if (findOpen && event.key === "Escape") {
          event.preventDefault();
          closeFind();
          return;
        }
        if (
          (event.ctrlKey || event.metaKey) &&
          !event.altKey &&
          event.key.toLowerCase() === "s"
        ) {
          event.preventDefault();
          void saveActiveEditor();
          return;
        }
        if (
          (event.ctrlKey || event.metaKey) &&
          !event.altKey &&
          !event.shiftKey &&
          event.key.toLowerCase() === "f"
        ) {
          event.preventDefault();
          openFind();
        }
      }}
    >
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
      {findOpen ? (
        <div className="editor-find" role="search" aria-label="Find in file">
          <label className="editor-find__query">
            <span className="sr-only">Find text</span>
            <input
              ref={findInputRef}
              type="text"
              aria-label="Find text"
              maxLength={MAX_FIND_QUERY_LENGTH}
              value={findQuery}
              onChange={(event) => {
                setFindQuery(
                  event.target.value.slice(0, MAX_FIND_QUERY_LENGTH),
                );
                setMatchCursor({ contextKey: findContextKey, index: 0 });
              }}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  moveMatch(event.shiftKey ? -1 : 1);
                }
              }}
            />
          </label>
          <span className="editor-find__status" role="status">
            {findStatus}
          </span>
          <button
            className="icon-button editor-find__button"
            type="button"
            aria-label="Previous match"
            disabled={findMatches.offsets.length === 0}
            onClick={() => moveMatch(-1)}
          >
            <ChevronUp size={14} aria-hidden="true" />
          </button>
          <button
            className="icon-button editor-find__button"
            type="button"
            aria-label="Next match"
            disabled={findMatches.offsets.length === 0}
            onClick={() => moveMatch(1)}
          >
            <ChevronDown size={14} aria-hidden="true" />
          </button>
          <button
            className="icon-button editor-find__button"
            type="button"
            aria-label="Match case"
            aria-pressed={caseSensitive}
            onClick={() => {
              setCaseSensitive((value) => !value);
              setMatchCursor({ contextKey: findContextKey, index: 0 });
            }}
          >
            <CaseSensitive size={14} aria-hidden="true" />
          </button>
          <button
            className="icon-button editor-find__button"
            type="button"
            aria-label="Close find"
            onClick={closeFind}
          >
            <X size={14} aria-hidden="true" />
          </button>
        </div>
      ) : null}
      <textarea
        ref={editorInputRef}
        className="editor-input"
        aria-label={`Edit ${path}`}
        value={editor.content}
        maxLength={MAX_EDITABLE_CONTENT_BYTES}
        disabled={editor.saving}
        spellCheck={false}
        onChange={(event) => updateEditorContent(event.target.value)}
      />
    </section>
  );
}
