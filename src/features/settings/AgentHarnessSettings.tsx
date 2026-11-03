import { useEffect, useRef, useState } from "react";
import { useWorkbenchStore } from "../../app/store";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import { errorMessage } from "../../domain/errors";
import { harnessAvailability } from "../chat/harnessAvailability";
import { LazyHarnessNotice } from "../chat/lazyParts";
import { SettingsRow } from "./SettingsRow";

export function AgentHarnessSettings() {
  const harnesses = useWorkbenchStore((state) => state.harnesses);
  const selectedId = useWorkbenchStore((state) => state.selectedHarnessId);
  const phase = useWorkbenchStore((state) => state.harnessDiscoveryPhase);
  const transitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const projectId = useWorkbenchStore((state) => state.project?.id);
  const selectHarness = useWorkbenchStore((state) => state.selectHarness);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const mounted = useRef(false);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  async function refresh() {
    const initialInventory = useWorkbenchStore.getState().harnesses;
    setRefreshing(true);
    setError(null);
    try {
      const inventory = await getWorkbenchPorts().agent.listHarnesses();
      const current = useWorkbenchStore.getState();
      if (mounted.current && current.harnesses === initialInventory)
        await current.applyHarnessDiscovery(inventory);
    } catch (cause) {
      if (mounted.current) setError(errorMessage(cause));
    } finally {
      if (mounted.current) setRefreshing(false);
    }
  }
  const disabled =
    transitioning || refreshing || phase === "checking" || phase === "waiting";
  return (
    <>
      <p>
        Installation and admission come from Sage's existing harness discovery.
        Sage does not install harnesses from this page.
      </p>
      <button
        className="button button--compact"
        type="button"
        disabled={disabled}
        onClick={() => void refresh()}
      >
        {refreshing ? "Checking harnesses..." : "Refresh discovery"}
      </button>
      {error ? <p role="alert">{error}</p> : null}
      <div className="agent-settings__card">
        {harnesses.slice(0, 32).map((harness) => {
          const availability = harnessAvailability(harness);
          const selected = harness.id === selectedId;
          const signIn = !harness.accountModelDiscovery
            ? "Sign-in status is not exposed by this harness."
            : !selected
              ? "Select this harness to check sign-in."
              : phase === "ready"
                ? "Signed in; account models discovered."
                : phase === "signInRequired" || phase === "consent"
                  ? "Sign-in required."
                  : phase === "waiting"
                    ? "Waiting for sign-in."
                    : phase === "checking"
                      ? "Checking sign-in..."
                      : "Sign-in status unknown.";
          return (
            <div key={harness.id}>
              <SettingsRow
                title={harness.name}
                description={`${availability.label}${harness.version ? ` · ${harness.version}` : ""}. ${availability.disabledReason ?? ""} ${signIn}`}
              >
                <button
                  className="button button--compact"
                  type="button"
                  disabled={
                    disabled ||
                    !projectId ||
                    Boolean(availability.disabledReason)
                  }
                  onClick={() => void selectHarness(harness.id)}
                >
                  {selected ? "Check models and sign-in" : "Select and check"}
                </button>
              </SettingsRow>
              {harness.signIn ? (
                <p>
                  {harness.signIn.hint} <code>{harness.signIn.command}</code>
                </p>
              ) : null}
            </div>
          );
        })}
      </div>
      {!harnesses.length ? <p role="status">No harnesses discovered.</p> : null}
      <LazyHarnessNotice />
    </>
  );
}
