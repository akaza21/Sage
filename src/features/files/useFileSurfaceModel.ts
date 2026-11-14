import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type RefObject,
} from "react";

import { tabsWithEditorUpdate } from "../../app/editorSlice";
import { useWorkbenchStore } from "../../app/store";
import { resourceKey, sameResource } from "../../domain/resourceIdentity";
import type { WorkbenchTab } from "../../domain/workbench";
import { MAX_FIND_QUERY_LENGTH, contentText } from "./editorContent";
import {
  useEditorExternalWatch,
  WATCH_MISSING_MESSAGE,
} from "./editorExternalWatch";
import type { EditorAdapterHandle } from "./EditorAdapter";
import type { FindControl, FindOpenRequest } from "./FileFindBar";
import { fileNavigationSelection } from "./fileNavigation";
import {
  getFileEditorController,
  useFileEditorLifecycle,
  type EditorStateBridge,
} from "./fileEditorController";

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
          tabs: tabsWithEditorUpdate(state.tabs, next.resource, update),
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
  const editorFocusBoundary = useCallback(
    () => editorInputRef.current?.view()?.dom ?? null,
    [editorInputRef],
  );
  const unwatchedMessage = useEditorExternalWatch(
    watchedResource,
    watchBridge,
    editorFocusBoundary,
  );
  const shownEditor = useMemo(
    () =>
      editor?.phase !== "ready" || !unwatchedMessage
        ? editor
        : unwatchedMessage === WATCH_MISSING_MESSAGE || editor.document.conflict
          ? { ...editor, unwatched: unwatchedMessage }
          : editor.message
            ? editor
            : { ...editor, message: unwatchedMessage },
    [editor, unwatchedMessage],
  );
  const [findRequest, setFindRequest] = useState<FindOpenRequest | null>(null);
  const findControlRef = useRef<FindControl | null>(null);
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
      contentText(editor.content),
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
    setFindRequest(null);
    editorInputRef.current?.focus();
  };
  const openFind = (replace = false) => {
    const view = editorInputRef.current?.view();
    const typing = findInputRef.current
      ?.closest(".editor-find")
      ?.contains(document.activeElement);
    const { from, to } = view?.state.selection.main ?? { from: 0, to: 0 };
    const selected = typing ? "" : (view?.state.sliceDoc(from, to) ?? "");
    const oneLine =
      selected.length <= MAX_FIND_QUERY_LENGTH && !selected.includes("\n");
    setFindRequest({ replace, seed: selected && oneLine ? selected : null });
  };
  return {
    resource,
    path,
    activeResourceKey,
    editor: shownEditor,
    projectTransitioning,
    updateEditorContent,
    saveActiveEditor,
    content,
    findOpen: findRequest !== null,
    openFind,
    closeFind,
    hideFind: () => setFindRequest(null),
    findControlRef,
    find: findRequest
      ? {
          request: findRequest,
          inputRef: findInputRef,
          controlRef: findControlRef,
          content,
          onClose: closeFind,
        }
      : null,
  };
}
