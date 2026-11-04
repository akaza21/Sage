import type { Text } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";
import { useEffect, useState } from "react";

import { useEditorSettingsStore } from "../../app/editorSettingsStore";
import { useWorkbenchStore, type WorkbenchState } from "../../app/store";
import type { ResourceRef } from "../../domain/workbench";
import type * as GitGutterModule from "./gitGutter";

const loadGitGutter = () => import("./gitGutter");

/** An editor, with the string and document it was first built from. */
export interface GitGutterEditor {
  view: EditorView;
  text: string | null;
  doc: Text;
}

/**
 * The editor's text without building it: the string it loaded from, while
 * its document is still the one built from that string. CodeMirror reads a
 * lone "\r" as a break, so such a string never stands for the document.
 */
function knownText({ view, text, doc }: GitGutterEditor) {
  return () =>
    text !== null && view.state.doc === doc && !text.includes("\r")
      ? text
      : null;
}

const rootOf = (state: WorkbenchState, rootId: string) =>
  state.repositories.roots.find((root) => root.rootId === rootId);

/**
 * Change markers for an Editor File in a repository (EP-14). The staged text
 * reloads after every Git refresh, which EP-12 runs on any outside change to
 * the index, even one that leaves the status unchanged (staging a second hunk
 * of an already staged File). One load runs at a time. Agent panes pass
 * `enabled: false`.
 */
export function useGitGutter(
  editor: GitGutterEditor | null,
  resource: ResourceRef | null,
  enabled: boolean,
) {
  const setting = useEditorSettingsStore(
    (state) => state.settings["editor.gitGutter"],
  );
  const rootId = resource?.rootId;
  const repository = useWorkbenchStore((state) =>
    rootId === undefined ? undefined : rootOf(state, rootId)?.state,
  );
  const active = enabled && setting && repository === "repository";
  const [gutter, setGutter] = useState<{
    handle: GitGutterModule.GitGutter;
    module: typeof GitGutterModule;
  } | null>(null);

  useEffect(() => {
    if (!editor || !active) return;
    let current = true;
    let handle: GitGutterModule.GitGutter | null = null;
    void loadGitGutter()
      .then((module) => {
        if (!current) return;
        handle = module.attachSharedGitGutter(editor.view, knownText(editor));
        setGutter({ handle, module });
      })
      .catch(() => undefined);
    return () => {
      current = false;
      handle?.destroy();
      setGutter(null);
    };
  }, [editor, active]);

  useEffect(() => {
    if (!gutter || !resource) return;
    return gutter.module.watchGitGutterBase(gutter.handle, resource);
  }, [gutter, resource]);
}
