import { useEffect } from "react";

import { useEditorSettingsStore } from "../app/editorSettingsStore";
import { startRepositoryAutoRefresh } from "../app/repositoryAutoRefresh";
import { useWorkbenchStore } from "../app/store";
import { subscribeWorkspaceNotifySearch } from "../features/files/workspaceNotifyCompose";
import { isTauriRuntime } from "../ports/nativeInvoke";
import { desktop } from "../services/desktop";

/** Refreshes Git state on outside changes while `git.autoRefresh` is on. */
export function useRepositoryAutoRefresh() {
  const enabled = useEditorSettingsStore(
    (store) => store.settings["git.autoRefresh"],
  );
  const projectId = useWorkbenchStore((state) => state.project?.id ?? null);
  useEffect(() => {
    // The browser runtime has no watcher and a fixed repository.
    if (!enabled || !projectId || !isTauriRuntime()) return;
    return startRepositoryAutoRefresh({
      refresh: async () => {
        // The store counts each refresh it starts; a busy store starts none.
        // An unchanged status keeps its object (`shareRepositoryStatus`).
        const before = useWorkbenchStore.getState();
        await before.refreshRepository();
        const after = useWorkbenchStore.getState();
        if (
          after.repositoryRequestGeneration ===
          before.repositoryRequestGeneration
        ) {
          return "skipped";
        }
        return after.repositories === before.repositories
          ? "unchanged"
          : "changed";
      },
      changeStamp: () => desktop.getRepositoryChangeStamp(),
      subscribeNotices: subscribeWorkspaceNotifySearch,
      // Only the Windows watcher sees every edit. macOS watches folders,
      // which misses a file edited in place, and the Linux and macOS watchers
      // stop at 64 folders, saying so only once, when the watch starts.
      pollAlways: !/windows/i.test(navigator.userAgent),
    });
  }, [enabled, projectId]);
}
