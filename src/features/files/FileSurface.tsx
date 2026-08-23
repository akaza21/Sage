import { useEffect, useMemo, useRef, useState } from "react";

import { useWorkbenchStore } from "../../app/store";
import { resourceKey, sameResource } from "../../domain/resourceIdentity";
import type { WorkbenchTab } from "../../domain/workbench";
import { EditorAdapter, type EditorAdapterHandle } from "./EditorAdapter";
import { FileEditorHeader } from "./FileEditorHeader";
import { FileEditorNotices } from "./FileEditorNotices";
import { FileFindBar } from "./FileFindBar";
import {
  getFileEditorController,
  useFileEditorLifecycle,
  type EditorStateBridge,
} from "./fileEditorController";
import { useEditorExternalWatch } from "./editorExternalWatch";
import { findLiteralMatches, MAX_EDITABLE_CONTENT_BYTES } from "./inFileFind";
import { fileNavigationSelection } from "./fileNavigation";
import { useLanguageSession } from "../language/languageClient";

export function FileSurface({ tab }: { tab: WorkbenchTab }) {
  const resource = tab.resource;
  const path = resource?.relativePath ?? tab.title;
  const activeResourceKey = resource ? resourceKey(resource) : "";
  const editorInputRef = useRef<EditorAdapterHandle>(null);
  const findInputRef = useRef<HTMLInputElement>(null);
  const selectedMatchRef = useRef<{ start: number; end: number } | null>(null);
  const [findOpen, setFindOpen] = useState(false);
  const [findQuery, setFindQuery] = useState("");
  const [caseSensitive, setCaseSensitive] = useState(false);
  const [matchCursor, setMatchCursor] = useState({
    contextKey: "",
    index: 0,
  });
  const fileEditorController = getFileEditorController();
  const editor = useWorkbenchStore((state) => state.activeEditor);
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
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
  const fileNavigationTarget = useWorkbenchStore(
    (state) => state.fileNavigationTarget,
  );
  const projectGeneration = useWorkbenchStore(
    (state) => state.projectGeneration,
  );
  const nativeTransitionEpoch = useWorkbenchStore(
    (state) => state.nativeTransitionEpoch,
  );
  const consumeFileNavigationTarget = useWorkbenchStore(
    (state) => state.consumeFileNavigationTarget,
  );
  const revision = editor?.phase === "ready" ? editor.revision : 0;
  const editorWatchBridge = useMemo<EditorStateBridge>(
    () => ({
      getActiveEditor: () => useWorkbenchStore.getState().activeEditor,
      setActiveEditor: (next, update) => {
        useWorkbenchStore.setState((state) => ({
          activeEditor: next,
          tabs:
            update?.dirty === undefined && update?.tabStatus === undefined
              ? state.tabs
              : state.tabs.map((candidate) =>
                  sameResource(candidate.resource, next.resource)
                    ? {
                        ...candidate,
                        ...(update?.dirty === undefined
                          ? {}
                          : { dirty: update.dirty }),
                        ...(update?.tabStatus === undefined
                          ? {}
                          : { status: update.tabStatus ?? undefined }),
                      }
                    : candidate,
                ),
        }));
      },
    }),
    [],
  );
  const watchedResource =
    editor?.phase === "ready" && sameResource(editor.resource, resource)
      ? resource
      : undefined;
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

  useFileEditorLifecycle(fileEditorController, {
    resource,
    editor,
    loadEditor,
    flushActiveEditor,
  });
  useEditorExternalWatch(watchedResource, editorWatchBridge);
  useLanguageSession(watchedResource, findContent);

  useEffect(() => {
    if (findOpen) {
      findInputRef.current?.focus();
      findInputRef.current?.select();
    }
  }, [findOpen]);

  useEffect(() => {
    const input = editorInputRef.current;
    if (
      !input ||
      !fileNavigationTarget ||
      projectTransitioning ||
      fileNavigationTarget.transitionEpoch !== nativeTransitionEpoch ||
      fileNavigationTarget.projectGeneration !== projectGeneration ||
      editor?.phase !== "ready" ||
      !sameResource(editor.resource, fileNavigationTarget.resource) ||
      !sameResource(resource, fileNavigationTarget.resource)
    ) {
      return;
    }
    const selection = fileNavigationSelection(
      editor.content,
      fileNavigationTarget,
    );
    input.focus();
    input.setSelection(selection.start, selection.end);
    consumeFileNavigationTarget(fileNavigationTarget);
  }, [
    consumeFileNavigationTarget,
    editor,
    fileNavigationTarget,
    nativeTransitionEpoch,
    projectGeneration,
    projectTransitioning,
    resource,
  ]);

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
        input.selection().start === selected.start &&
        input.selection().end === selected.end
      ) {
        input.setSelection(selected.start, selected.start);
      }
      selectedMatchRef.current = null;
      return;
    }
    const start = Math.min(offset, findContent.length);
    const end = Math.min(offset + findQuery.length, findContent.length);
    input.setSelection(start, end);
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
      <FileEditorHeader
        path={path}
        lineEnding={editor.document.lineEnding}
        diskByteLen={editor.document.disk.byteLen}
        persisting={editor.persisting}
        dirty={tab.dirty}
        saving={editor.saving}
        transitioning={projectTransitioning}
        onDiscard={() => void discardActiveEditor()}
        onSave={() => void saveActiveEditor()}
      />
      <FileEditorNotices conflict={conflict} message={editor.message} />
      {findOpen ? (
        <FileFindBar
          inputRef={findInputRef}
          query={findQuery}
          status={findStatus}
          caseSensitive={caseSensitive}
          hasMatches={findMatches.offsets.length > 0}
          onQueryChange={(query) => {
            setFindQuery(query);
            setMatchCursor({ contextKey: findContextKey, index: 0 });
          }}
          onMove={moveMatch}
          onToggleCase={() => {
            setCaseSensitive((value) => !value);
            setMatchCursor({ contextKey: findContextKey, index: 0 });
          }}
          onClose={closeFind}
        />
      ) : null}
      <EditorAdapter
        key={activeResourceKey}
        ref={editorInputRef}
        ariaLabel={`Edit ${path}`}
        path={path}
        content={editor.content}
        maxContentLength={MAX_EDITABLE_CONTENT_BYTES}
        disabled={editor.saving || projectTransitioning}
        onChange={updateEditorContent}
      />
    </section>
  );
}
