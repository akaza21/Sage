import type {
  BootstrapSnapshot,
  ProjectTransitionResult,
} from "../domain/workbench";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { validateTransitionIntent } from "./projectIntent";

export type LocalProjectTransitionHost = {
  projectId: string | null;
  nativeTransitionEpoch: number;
  begin: () => boolean;
  persistLayout: () => Promise<void>;
  setBooting: () => void;
  invalidate: () => number;
  applyCommitted: (
    result: ProjectTransitionResult,
    generation: number,
    validate: (snapshot: BootstrapSnapshot) => boolean,
    preserveChatOwnership: boolean,
  ) => Promise<void>;
  recoverUnknown: (
    generation: number,
    classify: (snapshot: BootstrapSnapshot) => "committed" | "unchanged" | null,
    preserveChatOwnership: boolean,
  ) => Promise<"committed" | "unchanged" | null>;
  fail: (error: unknown) => void;
  blockCommitted: (error: unknown) => void;
  idle: () => void;
  setUnchangedError: (error: unknown) => void;
};

export async function runEnsureLocalProject(
  host: LocalProjectTransitionHost,
): Promise<void> {
  if (!host.begin()) return;
  const previousProjectId = host.projectId;
  const expectedTransitionEpoch = host.nativeTransitionEpoch;
  let committed = false;
  let mutationInvoked = false;
  try {
    await host.persistLayout();
    host.setBooting();
    mutationInvoked = true;
    const result = await getWorkbenchPorts().project.ensureLocalProject(
      expectedTransitionEpoch,
    );
    validateTransitionIntent(result, expectedTransitionEpoch);
    if (result.committed) {
      committed = true;
      const generation = host.invalidate();
      await host.applyCommitted(
        result,
        generation,
        (snapshot) =>
          snapshot.project !== null &&
          (previousProjectId === null ||
            snapshot.project.id !== previousProjectId ||
            snapshot.project.roots.length === 0),
        false,
      );
    } else if (result.recoveryError) {
      host.fail(new Error(result.recoveryError));
    } else {
      host.idle();
    }
  } catch (error) {
    if (committed) {
      host.blockCommitted(error);
    } else if (mutationInvoked) {
      const generation = host.invalidate();
      try {
        const outcome = await host.recoverUnknown(
          generation,
          (snapshot) =>
            snapshot.project?.roots.length === 0
              ? "committed"
              : snapshot.project?.id === previousProjectId
                ? "unchanged"
                : previousProjectId === null
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
