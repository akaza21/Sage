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
