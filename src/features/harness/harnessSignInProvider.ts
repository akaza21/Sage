import { useWorkbenchStore } from "../../app/store";
import type { CodexAuthScope } from "../../domain/acp";
import { acpDesktop } from "../../services/acpDesktop";
import { acpHarnessDesktop } from "../../services/acpHarnessDesktop";

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
    const key = JSON.stringify(scope);
    const refreshRequirement = !scope
      ? "Open a workspace first."
      : state.selectedHarnessId !== harnessId
        ? `Select ${harness.name} to check sign-in.`
        : state.harnessDiscoveryPhase === "checking" ||
            state.pendingHarnessSignIn
          ? "Harness is busy."
          : null;
    return {
      key,
      backgroundCheck: false,
      refreshRequirement,
      status: async () => {
        const before = useWorkbenchStore.getState();
        if (!scope || signInProvider(harnessId)?.refreshRequirement !== null)
          return "unknown" as const;
        const reading = before.retryHarnessDiscovery();
        const requestGeneration =
          useWorkbenchStore.getState().harnessRequestGeneration;
        await reading;
        const current = useWorkbenchStore.getState();
        if (
          signInProvider(harnessId)?.key !== key ||
          current.selectedHarnessId !== harnessId ||
          current.harnessRequestGeneration !== requestGeneration
        )
          return "unavailable" as const;
        return current.harnessDiscoveryPhase === "ready"
          ? current.harnessAccountReadGeneration === requestGeneration &&
            accountEvidenceAfterRefusal(harnessId)
            ? ("signedIn" as const)
            : ("unavailable" as const)
          : current.harnessDiscoveryPhase === "signInRequired"
            ? ("signedOut" as const)
            : current.harnessDiscoveryPhase === "error"
              ? ("unavailable" as const)
              : ("unknown" as const);
      },
      start: async () => {
        if (!scope) throw new Error("Open a workspace to sign in.");
        if (useWorkbenchStore.getState().selectedHarnessId !== harnessId)
          await useWorkbenchStore.getState().selectHarness(harnessId);
        const current = useWorkbenchStore.getState();
        if (
          signInProvider(harnessId)?.key === key &&
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
      backgroundCheck: true,
      refreshRequirement: null,
      status: async () =>
        (await acpDesktop.getClaudeAuthStatus()).loggedIn
          ? ("signedIn" as const)
          : ("signedOut" as const),
      start: () => acpDesktop.startClaudeAuthLogin(),
    };
}

// Keep the last discovery observation across suspension. Replaying it on wake
// must not overwrite a newer protocol refusal with stale discovery state.
let accountObservation: readonly unknown[] = [];
// At most the 32 installed sign-in providers retain one refusal cutoff each.
const accountRefusals = new Map<string, { key: string; generation: number }>();

function accountEvidenceAfterRefusal(harnessId: string): boolean {
  const generation = useWorkbenchStore.getState().harnessAccountReadGeneration;
  const refusal = accountRefusals.get(harnessId);
  return (
    !refusal ||
    refusal.key !== signInProvider(harnessId)?.key ||
    (generation != null && generation > refusal.generation)
  );
}
function currentAccountObservation() {
  const state = useWorkbenchStore.getState();
  return [
    state.selectedHarnessId,
    signInProvider(state.selectedHarnessId ?? "")?.key,
    state.harnessAccount,
    state.harnessDiscoveryPhase,
    state.harnessRequestGeneration,
  ];
}

export function retainAccountSignInObservation(harnessId: string) {
  accountObservation = currentAccountObservation();
  for (const [id, refusal] of accountRefusals)
    if (signInProvider(id)?.key !== refusal.key) accountRefusals.delete(id);
  const provider = signInProvider(harnessId);
  if (provider)
    accountRefusals.set(harnessId, {
      key: provider.key,
      generation: useWorkbenchStore.getState().harnessRequestGeneration,
    });
}
export function resetAccountSignInForTest() {
  accountObservation = [];
  accountRefusals.clear();
}

/** Discovery and Settings feed the shared cache without another account read. */
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
    const observation = currentAccountObservation();
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
      // Ordinary discovery can replay native account state from before a refusal.
      else if (
        state.harnessDiscoveryPhase === "ready" &&
        state.harnessAccount &&
        accountEvidenceAfterRefusal(harness.id)
      )
        publish(harness.id, "signedIn");
    }
    changed();
  };
  const unsubscribe = useWorkbenchStore.subscribe(sync);
  sync();
  return unsubscribe;
}
