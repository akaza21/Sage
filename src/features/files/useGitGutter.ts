import type { EditorView } from "@codemirror/view";
import { useEffect, useState } from "react";

import { useEditorSettingsStore } from "../../app/editorSettingsStore";
import { useWorkbenchStore } from "../../app/store";
import { resourceKey } from "../../domain/resourceIdentity";
import type { ResourceRef } from "../../domain/workbench";
import type * as GitGutterModule from "./gitGutter";

const loadGitGutter = () => import("./gitGutter");

/**
 * The File's text while it is still the string it loaded as, which is then
 * the editor's document (a local edit makes the store hold the editor's
 * document instead). A lone "\r" reads differently in CodeMirror, so such
 * text is never offered.
 */
function loadedText(): string | null {
  // The File surface shows the active editor, so this is the view's File.
  const editor = useWorkbenchStore.getState().activeEditor;
  return editor?.phase === "ready" &&
    typeof editor.content === "string" &&
    !editor.content.includes("\r")
    ? editor.content
    : null;
}

/**
 * Change markers for an Editor File in a repository (EP-14). The staged text
 * reloads whenever the File's repository status changes (EP-12 keeps the
 * same object when nothing did), so a commit or stage made outside Sage
 * moves the markers. Agent panes pass `enabled: false`.
 */
export function useGitGutter(
  view: EditorView | null,
  resource: ResourceRef | null,
  enabled: boolean,
) {
  const setting = useEditorSettingsStore(
    (state) => state.settings["editor.gitGutter"],
  );
  const rootId = resource?.rootId;
  const repository = useWorkbenchStore((state) =>
    rootId === undefined
      ? undefined
      : state.repositories.roots.find((root) => root.rootId === rootId),
  );
  const active = enabled && setting && repository?.state === "repository";
  const [gutter, setGutter] = useState<{
    view: EditorView;
    handle: GitGutterModule.GitGutter;
    module: typeof GitGutterModule;
  } | null>(null);

  useEffect(() => {
    if (!view || !active) return;
    let current = true;
    let handle: GitGutterModule.GitGutter | null = null;
    void loadGitGutter()
      .then((module) => {
        if (!current) return;
        handle = module.attachSharedGitGutter(view, loadedText);
        setGutter({ view, handle, module });
      })
      .catch(() => undefined);
    return () => {
      current = false;
      handle?.destroy();
      setGutter(null);
    };
  }, [view, active]);

  const key = resource ? resourceKey(resource) : null;
  useEffect(() => {
    if (!gutter || !resource) return;
    let current = true;
    gutter.module
      .gutterBases()
      .load(resource)
      .then(
        (base) => current && gutter.handle.setBase(base),
        () => current && gutter.handle.setBase(null),
      );
    return () => {
      current = false;
    };
    // `repository` is the refresh signal; `resource` is named by `key`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [gutter, key, repository]);
}
