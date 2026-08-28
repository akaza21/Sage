import type {
  BootstrapSnapshot,
  ProjectTransitionResult,
} from "../domain/workbench";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { validateTransitionIntent } from "./projectIntent";
import { currentProjectLayout as layout } from "./projectProjection";
import type { WorkbenchState } from "./store";

export async function runSwitchProject(
  projectId: string,
  host: {
    get: () => WorkbenchState;
    begin: () => boolean;
    invalidate: () => number;
    setBooting: () => void;
    applyCommitted: (
      result: ProjectTransitionResult,
      generation: number,
      validate: (snapshot: BootstrapSnapshot) => boolean,
      preserveChatOwnership: boolean,
    ) => Promise<void>;
    recoverUnknown: (
      generation: number,
      classify: (
        snapshot: BootstrapSnapshot,
      ) => "committed" | "unchanged" | null,
      preserveChatOwnership: boolean,
    ) => Promise<"committed" | "unchanged" | null>;
    fail: (error: unknown) => void;
    blockCommitted: (error: unknown) => void;
    setUnchangedError: (error: unknown) => void;
  },
): Promise<void> {
  const current = layout(host.get());
  if (!current || current.projectId === projectId || !host.begin()) {
    return;
  }
  let committed = false;
  let mutationInvoked = false;
  const expectedTransitionEpoch = host.get().nativeTransitionEpoch;
  try {
    await host.get().flushActiveEditor();
    const generation = host.invalidate();
    host.setBooting();
    mutationInvoked = true;
    const result = await getWorkbenchPorts().project.switchProject(
      {
        targetProjectId: projectId,
        currentLayout: layout(host.get())!,
      },
      expectedTransitionEpoch,
    );
    validateTransitionIntent(result, expectedTransitionEpoch);
    committed = result.committed;
    await host.applyCommitted(
      result,
      generation,
      (snapshot) => snapshot.project?.id === projectId,
      false,
    );
  } catch (error) {
    if (committed) {
      host.blockCommitted(error);
    } else if (mutationInvoked) {
      const generation = host.get().projectGeneration;
      try {
        const outcome = await host.recoverUnknown(
          generation,
          (snapshot) =>
            snapshot.project?.id === projectId
              ? "committed"
              : snapshot.project?.id === current.projectId
                ? "unchanged"
                : null,
          false,
        );
        if (outcome === "unchanged") host.setUnchangedError(error);
      } catch (recoveryError) {
        host.blockCommitted(recoveryError);
      }
    } else {
      host.fail(error);
    }
  }
}
