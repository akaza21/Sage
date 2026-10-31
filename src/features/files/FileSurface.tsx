import { useCallback, useEffect, useRef, useState } from "react";

import { useEditorCaretPublisher } from "../../app/editorCaretStore";
import { useWorkbenchStore } from "../../app/store";
import { sameResource } from "../../domain/resourceIdentity";
import type { WorkbenchTab } from "../../domain/workbench";
import type { EditorAdapterHandle } from "./EditorAdapter";
import {
  subscribeEditorFocusRequest,
  takeEditorFocusRequest,
} from "./editorFocusRequest";
import { contentText, sameContent } from "./editorContent";
import {
  FileSurfaceContent,
  FileSurfaceResourceState,
} from "./FileSurfaceContent";
import {
  keymapBinding,
  matchesPlatformChord,
} from "../../domain/keymapRegistry";
import { usesMetaModifier } from "../../ui/shortcutLabel";
import {
  registerFileSurfaceCommands,
  type FileSurfaceCommand,
} from "./fileSurfaceCommandBus";
import { handleFileSurfaceCommandKey } from "./fileSurfaceCommands";
import { goToLineSelection } from "./goToLine";
import { canPreviewMarkdown } from "./markdownPreview";
import { localSymbolNavigation } from "../outline/outlineNavigation";
import { useFileSurfaceModel } from "./useFileSurfaceModel";

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
    content: findContent,
    findOpen,
    openFind,
    closeFind,
    hideFind,
    findControlRef,
    find,
  } = useFileSurfaceModel(tab, editorInputRef, findInputRef);
  const goToInputRef = useRef<HTMLInputElement>(null);
  const [previewSessionKey, setPreviewSessionKey] = useState("");
  const [sourceSessionKey, setSourceSessionKey] = useState("");
  const [goToSessionKey, setGoToSessionKey] = useState("");
  const [goToQuery, setGoToQuery] = useState("");
  const [goToError, setGoToError] = useState(false);
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

  useEffect(() => {
    if (goToOpen) {
      goToInputRef.current?.focus();
      goToInputRef.current?.select();
    }
  }, [goToOpen]);

  function openGoToLine() {
    hideFind();
    setGoToError(false);
    setGoToSessionKey(activeResourceKey);
  }

  function openFindFromSurface(replace = false) {
    setGoToSessionKey("");
    setGoToError(false);
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
    setGoToError(false);
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
      setGoToError(false);
      if (direction && findOpen && findControlRef.current) {
        findControlRef.current.move(direction);
      } else {
        openFind();
      }
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [findControlRef, findOpen, openFind, previewing, readOnly]);

  // The command palette (ED-2a) runs the same actions as the editor's keys.
  const runCommandRef = useRef<(command: FileSurfaceCommand) => void>(() => {});
  useEffect(() => {
    runCommandRef.current = (command) => {
      if (command === "save") void saveActiveEditor();
      else if (previewing) return;
      else if (command === "find") openFindFromSurface();
      else if (command === "replace") openFindFromSurface(true);
      else if (command === "goToLine") openGoToLine();
      else void goToLocalSymbol();
    };
  });
  useEffect(() => {
    if (readOnly) return;
    return registerFileSurfaceCommands((command) =>
      runCommandRef.current(command),
    );
  }, [readOnly]);

  function submitGoToLine() {
    const selection = goToLineSelection(contentText(findContent), goToQuery);
    if (!selection) {
      setGoToError(true);
      return;
    }
    editorInputRef.current?.setSelection(selection.start, selection.end);
    closeGoToLine();
  }

  async function goToLocalSymbol() {
    const input = editorInputRef.current;
    const readyEditor = editor;
    if (!input || !resource || readyEditor?.phase !== "ready") {
      setLanguageNavigationStatus("Local symbol navigation is unavailable.");
      return;
    }
    const { content } = readyEditor;
    const selection = input.selection();
    const { syntaxOutline } = await import("../outline/syntaxOutline");
    const text = contentText(content);
    const outline = await syntaxOutline(resource.relativePath, text);
    const stillCurrent = useWorkbenchStore.getState().activeEditor;
    if (
      stillCurrent?.phase !== "ready" ||
      !sameResource(stillCurrent.resource, resource) ||
      !sameContent(stillCurrent.content, content)
    ) {
      return;
    }
    const target = localSymbolNavigation(text, selection, outline);
    if (!target) {
      setLanguageNavigationStatus("No declaration in this File for that name.");
      return;
    }
    input.focus();
    input.setSelection(target.start, target.end);
    setLanguageNavigationStatus("Moved to local symbol.");
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
              inputRef: goToInputRef,
              query: goToQuery,
              status: goToError ? "Enter a line" : "Line or line:column",
              onQueryChange: (query) => {
                setGoToQuery(query);
                setGoToError(false);
              },
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
