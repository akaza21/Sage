import { useEditorSettingsStore } from "../../app/editorSettingsStore";
import { useWorkbenchStore } from "../../app/store";
import { rootGitDecorations, type RootGitDecorations } from "./gitDecorations";

/**
 * One root's Git marks from the EP-12 status, or null while `git.decorations`
 * is off. Refreshes keep an unchanged root's object, so this re-renders only
 * when that root's status changed.
 */
export function useRootGitDecorations(
  rootId: string,
): RootGitDecorations | null {
  const enabled = useEditorSettingsStore(
    (store) => store.settings["git.decorations"],
  );
  const root = useWorkbenchStore((state) =>
    enabled
      ? state.repositories.roots.find((item) => item.rootId === rootId)
      : undefined,
  );
  return root ? rootGitDecorations(root) : null;
}
