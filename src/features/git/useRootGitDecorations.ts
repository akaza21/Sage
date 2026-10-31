import { useEditorSettingsStore } from "../../app/editorSettingsStore";
import { useWorkbenchStore } from "../../app/store";
import type { RootGitDecorations } from "./gitDecorations";
import {
  useGitDecorationsUi,
  type GitDecorationsUiModule,
} from "./gitDecorationsGate";

export interface RootGitDecorationsView {
  decorations: RootGitDecorations;
  ui: GitDecorationsUiModule;
}

/**
 * One root's Git marks from the EP-12 status. Null while `git.decorations`
 * is off, outside a repository, or until the decoration chunk loads.
 * Refreshes keep an unchanged root's object, so this re-renders only when
 * that root's status changed.
 */
export function useRootGitDecorations(
  rootId: string,
): RootGitDecorationsView | null {
  const enabled = useEditorSettingsStore(
    (store) => store.settings["git.decorations"],
  );
  const root = useWorkbenchStore((state) =>
    enabled
      ? state.repositories.roots.find(
          (item) => item.rootId === rootId && item.state === "repository",
        )
      : undefined,
  );
  const ui = useGitDecorationsUi(root !== undefined);
  return root && ui ? { decorations: ui.rootGitDecorations(root), ui } : null;
}
