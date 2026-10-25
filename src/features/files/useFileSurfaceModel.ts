import { useEffect, useMemo, useRef, useState, type RefObject } from "react";

import { useWorkbenchStore } from "../../app/store";
import { resourceKey, sameResource } from "../../domain/resourceIdentity";
import type { WorkbenchTab } from "../../domain/workbench";
import { useEditorExternalWatch } from "./editorExternalWatch";
import type { EditorAdapterHandle } from "./EditorAdapter";
import { fileNavigationSelection } from "./fileNavigation";
import { findLiteralMatches } from "./inFileFind";
import {
  getFileEditorController,
  useFileEditorLifecycle,
  type EditorStateBridge,
} from "./fileEditorController";

/**
 * The File lifecycle/resource model deliberately owns store selection, load
 * ordering and external-watch state.  The surface
 * remains responsible only for mounted-view refs and command presentation.
 */
export function useFileSurfaceModel(
  tab: WorkbenchTab,
  editorInputRef: RefObject<EditorAdapterHandle | null>,
  findInputRef: RefObject<HTMLInputElement | null>,
) {
  const resource = tab.resource;
  const path = resource?.relativePath ?? tab.title;
  const activeResourceKey = resource ? resourceKey(resource) : "";
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
  const controller = getFileEditorController();
  const watchBridge = useMemo<EditorStateBridge>(
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
  const content =
    editor?.phase === "ready" && sameResource(editor.resource, resource)
      ? editor.content
      : "";
  useFileEditorLifecycle(controller, {
    resource,
    editor,
    loadEditor,
    flushActiveEditor,
  });
  useEditorExternalWatch(watchedResource, watchBridge);
  const selectedMatchRef = useRef<{ start: number; end: number } | null>(null);
  const [findOpen, setFindOpen] = useState(false);
  const [findQuery, setFindQuery] = useState("");
  const [caseSensitive, setCaseSensitive] = useState(false);
  const [matchCursor, setMatchCursor] = useState({ contextKey: "", index: 0 });
  const revision = editor?.phase === "ready" ? editor.revision : 0;
  const findContextKey = `${activeResourceKey}\0${revision}`;
  const findMatches = useMemo(
    () =>
      findOpen
        ? findLiteralMatches(content, findQuery, caseSensitive)
        : { offsets: [], truncated: false, contentLimitExceeded: false },
    [caseSensitive, content, findOpen, findQuery],
  );
  const resolvedMatchIndex =
    findMatches.offsets.length === 0
      ? 0
      : matchCursor.contextKey === findContextKey
        ? Math.min(matchCursor.index, findMatches.offsets.length - 1)
        : 0;
  useEffect(() => {
    if (findOpen) {
      findInputRef.current?.focus();
      findInputRef.current?.select();
    }
  }, [findInputRef, findOpen]);
  useEffect(() => {
    if (!findOpen) return;
    const input = editorInputRef.current;
    const offset = findMatches.offsets[resolvedMatchIndex];
    if (!input) return;
    if (offset === undefined) {
      const selected = selectedMatchRef.current;
      if (
        selected &&
        input.selection().start === selected.start &&
        input.selection().end === selected.end
      )
        input.setSelection(selected.start, selected.start);
      selectedMatchRef.current = null;
      return;
    }
    const start = Math.min(offset, content.length);
    const end = Math.min(offset + findQuery.length, content.length);
    input.setSelection(start, end);
    selectedMatchRef.current = { start, end };
  }, [
    content,
    editorInputRef,
    findMatches.offsets,
    findOpen,
    findQuery.length,
    resolvedMatchIndex,
  ]);
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
    editorInputRef,
    fileNavigationTarget,
    nativeTransitionEpoch,
    projectGeneration,
    projectTransitioning,
    resource,
  ]);
  const closeFind = () => {
    setFindOpen(false);
    setMatchCursor({ contextKey: findContextKey, index: 0 });
    selectedMatchRef.current = null;
    editorInputRef.current?.focus();
  };
  return {
    resource,
    path,
    activeResourceKey,
    editor,
    projectTransitioning,
    updateEditorContent,
    saveActiveEditor,
    content,
    findOpen,
    openFind: () => {
      if (findOpen) {
        findInputRef.current?.focus();
        findInputRef.current?.select();
        return;
      }
      setFindOpen(true);
    },
    closeFind,
    hideFind: () => setFindOpen(false),
    find: findOpen
      ? {
          inputRef: findInputRef,
          query: findQuery,
          status:
            findQuery.length === 0
              ? "No query"
              : findMatches.contentLimitExceeded
                ? "File exceeds the 1 MiB Find limit"
                : findMatches.offsets.length === 0
                  ? "No matches"
                  : `${resolvedMatchIndex + 1} of ${findMatches.offsets.length}${findMatches.truncated ? "+" : ""} ${findMatches.offsets.length === 1 && !findMatches.truncated ? "match" : "matches"}`,
          caseSensitive,
          hasMatches: findMatches.offsets.length > 0,
          onQueryChange: (query: string) => {
            setFindQuery(query);
            setMatchCursor({ contextKey: findContextKey, index: 0 });
          },
          onMove: (direction: 1 | -1) => {
            const count = findMatches.offsets.length;
            if (count === 0) return;
            setMatchCursor({
              contextKey: findContextKey,
              index: (resolvedMatchIndex + direction + count) % count,
            });
          },
          onToggleCase: () => {
            setCaseSensitive((value) => !value);
            setMatchCursor({ contextKey: findContextKey, index: 0 });
          },
          onClose: closeFind,
        }
      : null,
  };
}
