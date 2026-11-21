import { useWorkbenchStore } from "../../app/store";
import type { CodexAuthScope } from "../../domain/acp";
import { safeFailure } from "../../services/acpDesktopCommand";
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
          return { state: "unknown" as const, accountReadStartedAt: null };
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
          return { state: "unavailable" as const, accountReadStartedAt: null };
        return {
          state:
            current.harnessDiscoveryPhase === "ready"
              ? ("signedIn" as const)
              : current.harnessDiscoveryPhase === "signInRequired"
                ? ("signedOut" as const)
                : ("unavailable" as const),
          accountReadStartedAt: current.harnessAccountReadStartedAt,
        };
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
      status: async () => {
        try {
          const read = await acpDesktop.getClaudeAuthStatus();
          return {
            state: read.loggedIn
              ? ("signedIn" as const)
              : ("signedOut" as const),
            accountReadStartedAt: read.accountReadStartedAt,
          };
        } catch (error) {
          return {
            state: "unavailable" as const,
            accountReadStartedAt:
              safeFailure(error).accountReadStartedAt ?? null,
          };
        }
      },
      start: () => acpDesktop.startClaudeAuthLogin(),
    };
}

/** Discovery and Settings feed the shared cache without another account read. */
export function subscribeAccountSignIn(
  publish: (
    id: string,
    state: "signedIn" | "signedOut" | "unavailable",
    accountReadStartedAt: number,
  ) => void,
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
      state.harnessAccountReadStartedAt,
    ];
    if (next.every((value, index) => value === previous[index])) return;
    previous = next;
    const harness = signInHarnesses().find(
      (entry) => entry.id === state.selectedHarnessId,
    );
    if (
      state.project &&
      harness?.accountModelDiscovery &&
      harness.installState === "available" &&
      !state.projectTransitioning &&
      state.harnessAccountReadStartedAt != null
    ) {
      if (state.harnessDiscoveryPhase === "error")
        publish(harness.id, "unavailable", state.harnessAccountReadStartedAt);
      else if (
        state.harnessDiscoveryPhase === "signInRequired" ||
        state.harnessAccount === "signedOut"
      )
        publish(harness.id, "signedOut", state.harnessAccountReadStartedAt);
      else if (state.harnessDiscoveryPhase === "ready" && state.harnessAccount)
        publish(harness.id, "signedIn", state.harnessAccountReadStartedAt);
    }
    changed();
  };
  const unsubscribe = useWorkbenchStore.subscribe(sync);
  sync();
  return unsubscribe;
}
