import type {
  CodexAuthResponse,
  CodexAuthScope,
  CuratedBindingId,
  ProjectGeneration,
  SageProjectId,
} from "../domain/acp";
import { errorMessage } from "../domain/errors";
import { resetHarnessModelCache } from "./harnessModelCache";
import type { WorkbenchState } from "./store";

type Set = (partial: Partial<WorkbenchState>) => void;
type Get = () => WorkbenchState;

/**
 * Every sign-in and sign-out runs through here. Models listed for the
 * previous account are forgotten before the change, and any listed while it
 * ran are forgotten once it ends.
 *
 * A browser sign-in is `waiting` with the signed-out catalog still listed,
 * and one left unfinished asks for sign-in again. Any other change is
 * `checking` with no account until it is rediscovered, and one that does not
 * land is an error.
 */
export const accountChange =
  (set: Set, get: Get) =>
  async (
    call: (scope: CodexAuthScope) => Promise<CodexAuthResponse>,
    expected: CodexAuthResponse["phase"],
    pending: "checking" | "waiting",
  ) => {
    const scope = authScope(get());
    if (!scope) return;
    const requestGeneration = get().harnessRequestGeneration;
    resetHarnessModelCache();
    set(
      pending === "waiting"
        ? { harnessDiscoveryPhase: pending, harnessDiscoveryError: null }
        : {
            harnessDiscoveryPhase: pending,
            harnessDiscoveryError: null,
            harnessAccount: null,
            models: [],
            selectedModelId: null,
          },
    );
    try {
      const response = await call(scope).finally(resetHarnessModelCache);
      if (!authCurrent(get(), scope, requestGeneration)) return;
      if (response.phase !== expected)
        set(
          pending === "waiting"
            ? { harnessDiscoveryPhase: "signInRequired" }
            : {
                harnessDiscoveryPhase: "error",
                harnessDiscoveryError:
                  "The Codex account change did not converge safely.",
              },
        );
      else if (expected === "signedOut")
        set({ harnessDiscoveryPhase: "signInRequired" });
      else await get().retryHarnessDiscovery();
    } catch (error) {
      if (!authCurrent(get(), scope, requestGeneration)) return;
      set({
        harnessDiscoveryPhase: "error",
        harnessDiscoveryError: errorMessage(error),
      });
    }
  };

export function authScope(state: WorkbenchState) {
  const harness = state.harnesses.find(
    (candidate) => candidate.id === state.selectedHarnessId,
  );
  if (
    !state.project ||
    harness?.transport !== "acp" ||
    !harness.accountModelDiscovery ||
    !harness.bindingId
  ) {
    return null;
  }
  return {
    projectId: state.project.id as SageProjectId,
    projectGeneration: String(state.nativeTransitionEpoch) as ProjectGeneration,
    bindingId: harness.bindingId as CuratedBindingId,
  };
}

export function authCurrent(
  state: WorkbenchState,
  scope: NonNullable<ReturnType<typeof authScope>>,
  requestGeneration: number,
) {
  return (
    state.project?.id === scope.projectId &&
    String(state.nativeTransitionEpoch) === scope.projectGeneration &&
    state.harnessRequestGeneration === requestGeneration &&
    state.harnesses.find(
      (candidate) => candidate.id === state.selectedHarnessId,
    )?.bindingId === scope.bindingId
  );
}
