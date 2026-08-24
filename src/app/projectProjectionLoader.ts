import type { StoreApi } from "zustand";

import { childResource } from "../domain/resourceIdentity";
import type { BootstrapSnapshot } from "../domain/workbench";
import { projectChatOwnership } from "./chatOwnershipProjection";
import { matchesProjectIntent, nextProjectGeneration } from "./projectIntent";
import { snapshotProjection } from "./projectProjection";
import type { WorkbenchState } from "./store";
import { bindWorkspaceNotifyFromState } from "./workspaceNotifyLifecycle";

export async function loadProjectProjection(
  set: StoreApi<WorkbenchState>["setState"],
  get: StoreApi<WorkbenchState>["getState"],
  snapshot: BootstrapSnapshot,
  generation: number,
  preserveChatOwnership: boolean,
) {
  const projectId = snapshot.project?.id ?? null;
  set({
    ...snapshotProjection(snapshot),
    ...projectChatOwnership(get(), snapshot, preserveChatOwnership),
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
    try {
      await bindWorkspaceNotifyFromState(get);
    } catch {
      /* watch failure must not fail Project projection */
    }
  }
}
