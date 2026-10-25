import { useEffect, useRef, useState } from "react";

import { useEditorCaretPublisher } from "../../app/editorCaretStore";
import { useWorkbenchStore } from "../../app/store";
import { sameResource } from "../../domain/resourceIdentity";
import type { WorkbenchTab } from "../../domain/workbench";
import type { EditorAdapterHandle } from "./EditorAdapter";
import {
  FileSurfaceContent,
  FileSurfaceResourceState,
} from "./FileSurfaceContent";
import { handleFileSurfaceCommandKey } from "./fileSurfaceCommands";
import { goToLineSelection } from "./goToLine";
import { canPreviewMarkdown } from "./markdownPreview";
import { localSymbolNavigation } from "../outline/outlineNavigation";
import { useFileSurfaceModel } from "./useFileSurfaceModel";

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

  const [languageNavigationStatus, setLanguageNavigationStatus] = useState("");
  const reviewReadOnly = readOnly && !projectTransitioning;
  const onCaretOffset = useEditorCaretPublisher(
    !previewing &&
      editor?.phase === "ready" &&
      resource &&
      sameResource(editor.resource, resource)
      ? editor.content
      : null,
  );

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

  function openFindFromSurface() {
    setGoToSessionKey("");
    setGoToError(false);
    openFind();
  }

  function closeGoToLine() {
    setGoToSessionKey("");
    setGoToError(false);
    editorInputRef.current?.focus();
  }

  useEffect(() => {
    if (!readOnly || previewing) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (
        event.key.toLowerCase() !== "f" ||
        (!event.ctrlKey && !event.metaKey) ||
        event.altKey
      )
        return;
      event.preventDefault();
      setGoToSessionKey("");
      setGoToError(false);
      openFind();
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [openFind, previewing, readOnly]);

  function submitGoToLine() {
    const selection = goToLineSelection(findContent, goToQuery);
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
    const outline = await syntaxOutline(resource.relativePath, content);
    const stillCurrent = useWorkbenchStore.getState().activeEditor;
    if (
      stillCurrent?.phase !== "ready" ||
      !sameResource(stillCurrent.resource, resource) ||
      stillCurrent.content !== content
    ) {
      return;
    }
    const target = localSymbolNavigation(content, selection, outline);
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
      agentMarkdown={agentMarkdown}
      onCaretOffset={onCaretOffset}
      onEditorChange={readOnly ? () => undefined : updateEditorContent}
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
          openFind: openFindFromSurface,
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
