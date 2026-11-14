import type { StoreApi } from "zustand";

import { childResource } from "../domain/resourceIdentity";
import type { BootstrapSnapshot } from "../domain/workbench";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { projectChatOwnership } from "./chatOwnershipProjection";
import { matchesProjectIntent, nextProjectGeneration } from "./projectIntent";
import {
  reconcileLiveBrowserTabs,
  snapshotProjection,
} from "./projectProjection";
import type { WorkbenchState } from "./store";

export async function loadProjectProjection(
  set: StoreApi<WorkbenchState>["setState"],
  get: StoreApi<WorkbenchState>["getState"],
  snapshot: BootstrapSnapshot,
  generation: number,
  preserveChatOwnership: boolean,
) {
  snapshot = await reconcileLiveBrowserTabs(snapshot);
  if (get().projectGeneration !== generation) return;
  const projectId = snapshot.project?.id ?? null;
  const restoredQueues =
    projectId && (!preserveChatOwnership || get().project?.id !== projectId)
      ? await getWorkbenchPorts().project.getProjectChatQueuedPrompts(projectId)
      : {};
  if (get().projectGeneration !== generation) return;
  set({
    ...snapshotProjection(snapshot),
    ...projectChatOwnership(
      get(),
      snapshot,
      preserveChatOwnership,
      restoredQueues,
    ),
    projectGeneration: generation,
    repositoryRequestGeneration: nextProjectGeneration(
      get().repositoryRequestGeneration,
    ),
    harnessRequestGeneration: nextProjectGeneration(
      get().harnessRequestGeneration,
    ),
    projectTransitioning: true,
  });
  const availableRoots =
    snapshot.project?.roots.filter(
      (root) => root.availability === "available",
    ) ?? [];
  const projectionLoads: Promise<void>[] = availableRoots.map((root) =>
    get().loadDirectory(childResource(root.id)),
  );
  if (projectId && snapshot.selectedHarnessId) {
    projectionLoads.push(
      get().loadHarnessModelsForProjection(
        snapshot.selectedHarnessId,
        snapshot.selectedModelId,
        generation,
        projectId,
      ),
    );
  }
  await Promise.all(projectionLoads);
  if (projectId && !matchesProjectIntent(get(), generation, projectId)) {
    return;
  }
  const active = snapshot.tabs.find((tab) => tab.id === snapshot.activeTabId);
  if (active?.kind === "file" && active.resource) {
    await get().loadEditor(active.resource);
  } else if (active?.kind === "diff" && active.resource) {
    await get().loadDiff(active.resource);
  }
  if (!projectId || matchesProjectIntent(get(), generation, projectId)) {
    set({ projectTransitioning: false });
    const activeResource =
      (active?.kind === "file" || active?.kind === "diff") && active.resource
        ? active.resource
        : null;
    if (
      activeResource &&
      availableRoots.some((root) => root.id === activeResource.rootId)
    ) {
      await get().revealFile(activeResource);
      if (
        get().projectTransitioning ||
        (projectId && !matchesProjectIntent(get(), generation, projectId))
      ) {
        return;
      }
    }
    try {
      // The watcher loads with the first Project, off the startup path.
      const { bindWorkspaceNotifyFromState } =
        await import("./workspaceNotifyLifecycle");
      await bindWorkspaceNotifyFromState(get);
    } catch {
      /* watch failure must not fail Project projection */
    }
  }
}
