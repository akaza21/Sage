import { useEffect } from "react";

import { useEditorSettingsStore } from "../app/editorSettingsStore";
import { startRepositoryAutoRefresh } from "../app/repositoryAutoRefresh";
import { useWorkbenchStore } from "../app/store";
import { subscribeWorkspaceNotifySearch } from "../features/files/workspaceNotifyCompose";
import { isTauriRuntime } from "../ports/nativeInvoke";
import { desktop } from "../services/desktop";
import { usesMetaModifier } from "../ui/shortcutLabel";

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
      refresh: () => useWorkbenchStore.getState().refreshRepository(),
      changeStamp: () => desktop.getRepositoryChangeStamp(),
      subscribeNotices: subscribeWorkspaceNotifySearch,
      subscribeRefreshing: (listener) =>
        useWorkbenchStore.subscribe((state, previous) => {
          if (state.repositoryRefreshing !== previous.repositoryRefreshing) {
            listener(state.repositoryRefreshing);
          }
        }),
      // macOS watches folders, which misses edits made in place.
      pollAlways: usesMetaModifier(),
    });
  }, [enabled, projectId]);
}
