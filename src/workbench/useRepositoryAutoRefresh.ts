import { useEffect } from "react";

import { useEditorSettingsStore } from "../app/editorSettingsStore";
import { startRepositoryAutoRefresh } from "../app/repositoryAutoRefresh";
import { useWorkbenchStore } from "../app/store";
import { subscribeWorkspaceNotifyRepository } from "../features/files/workspaceNotifyCompose";
import { isTauriRuntime } from "../ports/nativeInvoke";
import { desktop } from "../services/desktop";

export function useRepositoryAutoRefresh() {
  const enabled = useEditorSettingsStore(
    (store) => store.settings["git.autoRefresh"],
  );
  const projectId = useWorkbenchStore((state) => state.project?.id ?? null);
  useEffect(() => {
    if (!enabled || !projectId || !isTauriRuntime()) return;
    return startRepositoryAutoRefresh({
      refresh: async () => {
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
      subscribeNotices: subscribeWorkspaceNotifyRepository,
      // Only the Windows watcher sees every edit. macOS watches folders,
      // which misses a file edited in place, and the Linux and macOS watchers
      // stop at 64 folders, saying so only once, when the watch starts.
      pollAlways: !/windows/i.test(navigator.userAgent),
    });
  }, [enabled, projectId]);
}
