import { useWorkbenchStore } from "../../app/store";
import type { CodexAuthScope } from "../../domain/acp";
import { acpDesktop } from "../../services/acpDesktop";
import { acpHarnessDesktop } from "../../services/acpHarnessDesktop";
import { codexAuthStatusDesktop } from "../../services/codexAuthStatusDesktop";

export function signInHarnesses() {
  return useWorkbenchStore.getState().harnesses.slice(0, 32);
}

/** Native installation capabilities select adapters; IDs only identify entries. */
export function signInProvider(harnessId: string) {
  const state = useWorkbenchStore.getState();
  const harness = signInHarnesses().find((entry) => entry.id === harnessId);
  if (harness?.installState !== "available") return;
  if (harness.accountModelDiscovery && harness.bindingId) {
    const scope =
      state.project && !state.projectTransitioning
        ? ({
            projectId: state.project.id,
            projectGeneration: String(state.nativeTransitionEpoch),
            bindingId: harness.bindingId,
          } as CodexAuthScope)
        : null;
    return {
      key: JSON.stringify(scope),
      status: async () => {
        if (!scope) return "unknown" as const;
        const requestGeneration =
          useWorkbenchStore.getState().harnessRequestGeneration;
        const result = await codexAuthStatusDesktop.getAuth(scope);
        const current = useWorkbenchStore.getState();
        if (
          result.phase === "signedIn" &&
          signInProvider(harnessId)?.key === JSON.stringify(scope) &&
          current.selectedHarnessId === harnessId &&
          current.harnessRequestGeneration === requestGeneration &&
          current.harnessDiscoveryPhase === "signInRequired"
        )
          await current.retryHarnessDiscovery();
        return result.phase === "signedIn"
          ? ("signedIn" as const)
          : result.phase === "signedOut" || result.phase === "recovery"
            ? ("signedOut" as const)
            : ("unknown" as const);
      },
      start: async () => {
        if (!scope) throw new Error("Open a workspace to sign in.");
        if (useWorkbenchStore.getState().selectedHarnessId !== harnessId)
          await useWorkbenchStore.getState().selectHarness(harnessId);
        const current = useWorkbenchStore.getState();
        if (
          signInProvider(harnessId)?.key === JSON.stringify(scope) &&
          current.selectedHarnessId === harnessId
        )
          await current.signInHarnessWith(
            acpHarnessDesktop.startCodexAuth,
            "waiting",
          );
      },
    };
  }
  if (harness.signIn?.statusCheck === "claudeCli")
    return {
      key: JSON.stringify([harness.bindingId, harness.version]),
      status: async () =>
        (await acpDesktop.getClaudeAuthStatus()).loggedIn
          ? ("signedIn" as const)
          : ("signedOut" as const),
      start: () => acpDesktop.startClaudeAuthLogin(),
    };
}

// Keep the last discovery observation across suspension. Replaying it on wake
// must not overwrite a newer status check with stale model-discovery state.
let accountObservation: readonly unknown[] = [];
export function resetAccountSignInForTest() {
  accountObservation = [];
}

/** Settings account changes feed the same cache as background status checks. */
export function subscribeAccountSignIn(
  publish: (id: string, state: "signedIn" | "signedOut") => void,
  changed: () => void,
) {
  let previous: readonly unknown[] = [];
  const sync = () => {
    const state = useWorkbenchStore.getState();
    const next = [
      state.harnesses,
      state.selectedHarnessId,
      state.project?.id,
      state.nativeTransitionEpoch,
      state.projectTransitioning,
      state.harnessAccount,
      state.harnessDiscoveryPhase,
      state.harnessRequestGeneration,
    ];
    if (next.every((value, index) => value === previous[index])) return;
    previous = next;
    const harness = signInHarnesses().find(
      (entry) => entry.id === state.selectedHarnessId,
    );
    const observation = [
      harness?.id,
      harness && signInProvider(harness.id)?.key,
      state.harnessAccount,
      state.harnessDiscoveryPhase,
      state.harnessRequestGeneration,
    ];
    const newObservation = observation.some(
      (value, index) => value !== accountObservation[index],
    );
    accountObservation = observation;
    if (
      newObservation &&
      state.project &&
      harness?.accountModelDiscovery &&
      harness.installState === "available" &&
      !state.projectTransitioning
    ) {
      if (
        state.harnessDiscoveryPhase === "signInRequired" ||
        state.harnessAccount === "signedOut"
      )
        publish(harness.id, "signedOut");
      else if (state.harnessDiscoveryPhase === "ready" && state.harnessAccount)
        publish(harness.id, "signedIn");
    }
    changed();
  };
  const unsubscribe = useWorkbenchStore.subscribe(sync);
  sync();
  return unsubscribe;
}
