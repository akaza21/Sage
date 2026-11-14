import type {
  CodexAuthResponse,
  CodexAuthScope,
  CuratedBindingId,
  ProjectGeneration,
  SageProjectId,
} from "../domain/acp";
import { errorMessage } from "../domain/errors";
import type { WorkbenchState } from "./store";

type Set = (partial: Partial<WorkbenchState>) => void;
type Get = () => WorkbenchState;

export const accountChange =
  (set: Set, get: Get) =>
  async (
    call: (scope: CodexAuthScope) => Promise<CodexAuthResponse>,
    expected: CodexAuthResponse["phase"],
  ) => {
    const scope = authScope(get());
    if (!scope) return;
    const requestGeneration = get().harnessRequestGeneration;
    set({
      harnessDiscoveryPhase: "checking",
      harnessDiscoveryError: null,
      harnessAccount: null,
      models: [],
      selectedModelId: null,
    });
    try {
      const response = await call(scope);
      if (!authCurrent(get(), scope, requestGeneration)) return;
      if (response.phase !== expected)
        set({
          harnessDiscoveryPhase: "error",
          harnessDiscoveryError:
            "The Codex account change did not converge safely.",
        });
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
