import type { EditorView } from "@codemirror/view";
import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useEffectEvent,
  useRef,
  useState,
} from "react";

import { useEditorCaretPublisher } from "../../app/editorCaretStore";
import { watchEditorHistory } from "../../app/editorHistory";
import { useWorkbenchStore } from "../../app/store";
import {
  keymapBinding,
  matchesPlatformChord,
} from "../../domain/keymapRegistry";
import { sameResource } from "../../domain/resourceIdentity";
import type { WorkbenchTab } from "../../domain/workbench";
import { usesMetaModifier } from "../../ui/shortcutLabel";
import type { EditorAdapterHandle } from "./EditorAdapter";
import {
  subscribeEditorFocusRequest,
  takeEditorFocusRequest,
} from "./editorFocusRequest";
import { registerReplaceEditor } from "./editorTextReplace";
import {
  registerFileSurfaceCommands,
  type FileSurfaceCommands,
} from "./fileSurfaceCommandBus";
import { handleFileSurfaceCommandKey } from "./fileSurfaceCommands";
import {
  FileSurfaceContent,
  FileSurfaceResourceState,
} from "./FileSurfaceContent";
import { canPreviewMarkdown } from "./filePreviewKind";
import { useFileSurfaceModel } from "./useFileSurfaceModel";
import { useGitGutter, type GitGutterEditor } from "./useGitGutter";

const MixedEolFileView = lazy(() =>
  import("./MixedEolFileView").then((module) => ({
    default: module.MixedEolFileView,
  })),
);

const LIMIT_EXCEEDED_STATUS =
  "That edit was not applied: it would make this File larger than 1 MiB, the editor's limit.";

export function FileSurface({
  tab,
  readOnly = false,
}: {
  tab: WorkbenchTab;
  readOnly?: boolean;
}) {
  const editorInputRef = useRef<EditorAdapterHandle>(null);
  const projectName = useWorkbenchStore((state) => state.project?.name);
  const findInputRef = useRef<HTMLInputElement>(null);
  const {
    resource,
    path,
    activeResourceKey,
    editor,
    projectTransitioning,
    updateEditorContent,
    saveActiveEditor,
    findOpen,
    openFind,
    closeFind,
    hideFind,
    findControlRef,
    find,
  } = useFileSurfaceModel(tab, editorInputRef, findInputRef);
  const [previewSessionKey, setPreviewSessionKey] = useState("");
  const [sourceSessionKey, setSourceSessionKey] = useState("");
  const [goToSessionKey, setGoToSessionKey] = useState("");
  const [goToQuery, setGoToQuery] = useState("");
  const agentMarkdown = readOnly && canPreviewMarkdown(path);
  const previewing = agentMarkdown
    ? sourceSessionKey !== activeResourceKey
    : previewSessionKey === activeResourceKey;
  const goToOpen = goToSessionKey === activeResourceKey;

  // A status belongs to the File it was written for and is dropped on switch.
  const [status, setStatus] = useState({ key: "", text: "" });
  const languageNavigationStatus =
    status.key === activeResourceKey ? status.text : "";
  const setLanguageNavigationStatus = useCallback(
    (text: string) => setStatus({ key: activeResourceKey, text }),
    [activeResourceKey],
  );
  const reviewReadOnly = readOnly && !projectTransitioning;
  const onCaretOffset = useEditorCaretPublisher(
    !previewing &&
      editor?.phase === "ready" &&
      !!resource &&
      sameResource(editor.resource, resource),
    !readOnly,
  );

  const reportLimitExceeded = useCallback(
    () => setLanguageNavigationStatus(LIMIT_EXCEEDED_STATUS),
    [setLanguageNavigationStatus],
  );
  // The next edit that lands shows the refusal no longer applies.
  const editorChanged = useCallback<typeof updateEditorContent>(
    (...change) => {
      setStatus((current) =>
        current.text === LIMIT_EXCEEDED_STATUS
          ? { key: "", text: "" }
          : current,
      );
      return updateEditorContent(...change);
    },
    [updateEditorContent],
  );

  // A pending "focus the opened File" request (New File > Create) is taken
  // once this File's editor is on screen, after the render that shows it.
  const editorShown =
    !readOnly &&
    !previewing &&
    editor?.phase === "ready" &&
    !!resource &&
    sameResource(editor.resource, resource);
  // The editor, with the File it was made for, so markers never cross Files.
  const [gutterEditor, setGutterEditor] = useState<
    (GitGutterEditor & { key: string }) | null
  >(null);
  const onEditorView = useCallback(
    (view: EditorView | null, text: string | null) =>
      setGutterEditor(
        view && { view, text, doc: view.state.doc, key: activeResourceKey },
      ),
    [activeResourceKey],
  );
  useGitGutter(
    gutterEditor?.key === activeResourceKey ? gutterEditor : null,
    resource ?? null,
    editorShown,
  );
  // A workspace Replace changes this File's unsaved text here, as one edit
  // the user can undo.
  const replaceView =
    !readOnly && gutterEditor?.key === activeResourceKey ? gutterEditor : null;
  useEffect(() => {
    if (replaceView)
      return registerReplaceEditor(replaceView.key, replaceView.view);
  }, [replaceView]);
  const [focusRequests, setFocusRequests] = useState(0);
  useEffect(
    () => subscribeEditorFocusRequest(() => setFocusRequests((n) => n + 1)),
    [],
  );
  useEffect(() => {
    if (editorShown && takeEditorFocusRequest(resource)) {
      editorInputRef.current?.focus();
    }
  }, [resource, editorShown, focusRequests]);

  function openGoToLine() {
    hideFind();
    setGoToSessionKey(activeResourceKey);
  }

  function openFindFromSurface(replace = false) {
    setGoToSessionKey("");
    openFind(replace);
  }

  function moveFind(direction: 1 | -1) {
    if (findOpen && findControlRef.current) {
      findControlRef.current.move(direction);
    } else {
      openFindFromSurface();
    }
  }

  function closeGoToLine() {
    setGoToSessionKey("");
    editorInputRef.current?.focus();
  }

  useEffect(() => {
    if (!readOnly || previewing) return;
    const onKeyDown = (event: KeyboardEvent) => {
      const apple = usesMetaModifier();
      const matches = (id: string) =>
        matchesPlatformChord(keymapBinding(id), event, apple);
      const direction = matches("findNext")
        ? 1
        : matches("findPrevious")
          ? -1
          : 0;
      if (!direction && !matches("find")) return;
      event.preventDefault();
      setGoToSessionKey("");
      if (direction && findOpen && findControlRef.current) {
        findControlRef.current.move(direction);
      } else {
        openFind();
      }
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [findControlRef, findOpen, openFind, previewing, readOnly]);

  // Go back and Go forward (ED-2b) follow the editable File's caret.
  useEffect(() => {
    if (!readOnly) watchEditorHistory();
  }, [readOnly]);

  // The command palette (ED-2a) runs the same actions as the editor's keys.
  const availableCommand = useRef<FileSurfaceCommands["available"]>(
    () => false,
  );
  useEffect(() => {
    // Availability is queried during command-palette rendering, so it reads
    // the last committed editor state rather than invoking an Effect Event.
    availableCommand.current = (command) =>
      editor?.phase === "ready" && (command === "save" || !previewing);
  });
  const runCommand = useEffectEvent<FileSurfaceCommands["run"]>((command) => {
    if (command === "save") void saveActiveEditor();
    else if (command === "find") openFindFromSurface();
    else if (command === "replace") openFindFromSurface(true);
    else if (command === "goToLine") openGoToLine();
    else void goToLocalSymbol();
  });
  useEffect(() => {
    if (readOnly) return;
    return registerFileSurfaceCommands({
      available: (command) => availableCommand.current(command),
      run: (command) => runCommand(command),
    });
  }, [readOnly]);

  function submitGoToLine(selection: { start: number; end: number }) {
    editorInputRef.current?.setSelection(selection.start, selection.end);
    closeGoToLine();
  }

  async function goToLocalSymbol() {
    const input = editorInputRef.current;
    const selection = input?.selection();
    const { navigateFileSymbol } = await import("./fileSymbolNavigation");
    const message = await navigateFileSymbol(
      input,
      resource,
      editor,
      selection,
    );
    if (message) setLanguageNavigationStatus(message);
  }

  if (
    !resource ||
    !editor ||
    !sameResource(editor.resource, resource) ||
    editor.phase === "loading"
  ) {
    return <FileSurfaceResourceState path={path} />;
  }
  if (editor.phase === "error") {
    if (editor.kind === "mixedLineEndings")
      return (
        <Suspense fallback={<p role="status">Loading file viewer...</p>}>
          <MixedEolFileView key={activeResourceKey} resource={resource} />
        </Suspense>
      );
    return <FileSurfaceResourceState path={path} error={editor.message} />;
  }

  return (
    <FileSurfaceContent
      activeResourceKey={activeResourceKey}
      dirty={tab.dirty}
      editor={editor}
      editorInputRef={editorInputRef}
      find={find}
      goTo={
        goToOpen && !previewing
          ? {
              query: goToQuery,
              onQueryChange: setGoToQuery,
              onSubmit: submitGoToLine,
              onClose: closeGoToLine,
            }
          : null
      }
      languageNavigationStatus={languageNavigationStatus}
      path={path}
      projectName={readOnly ? projectName : undefined}
      previewing={previewing}
      transitioning={projectTransitioning}
      readOnly={reviewReadOnly}
      reviewOnly={readOnly}
      agentMarkdown={agentMarkdown}
      onCaretOffset={onCaretOffset}
      onLimitExceeded={reportLimitExceeded}
      onEditorView={onEditorView}
      onEditorChange={readOnly ? () => undefined : editorChanged}
      onSurfaceKeyDown={(event) => {
        handleFileSurfaceCommandKey(event, {
          previewing,
          findOpen,
          goToOpen,
          closePreview: () => {
            setPreviewSessionKey("");
            hideFind();
            setGoToSessionKey("");
          },
          closeFind,
          closeGoToLine,
          openFind: () => openFindFromSurface(),
          openReplace: () => openFindFromSurface(true),
          moveFind,
          openGoToLine,
          goToLocalSymbol: () => void goToLocalSymbol(),
          save: readOnly ? () => undefined : () => void saveActiveEditor(),
        });
      }}
      onTogglePreview={() => {
        hideFind();
        setGoToSessionKey("");
        if (agentMarkdown) {
          setSourceSessionKey((current) =>
            current === activeResourceKey ? "" : activeResourceKey,
          );
        } else {
          setPreviewSessionKey((current) =>
            current === activeResourceKey ? "" : activeResourceKey,
          );
        }
      }}
    />
  );
}

export default FileSurface;
