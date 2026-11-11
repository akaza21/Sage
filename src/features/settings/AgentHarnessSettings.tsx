import { useState } from "react";
import { useWorkbenchStore } from "../../app/store";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import { errorMessage } from "../../domain/errors";
import { harnessAvailability } from "../chat/harnessAvailability";
import { LazyHarnessNotice } from "../chat/lazyParts";
import { LazyModuleBoundary } from "../../ui/LazyModuleBoundary";
import { CodexAccountSettings } from "./CodexAccountSettings";
import { SettingsRow } from "./SettingsRow";

// The user-defined agent editor loads only when Harnesses is open, below
// the harness list, so it arrives without a loading notice.
const loadCustomAgentSettings = () => import("./CustomAgentSettings");

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
  // A refresh finishes whether or not Settings is still on screen: hidden
  // while the window is minimized, React keeps this page's state, so the
  // result and the end of refreshing are recorded for wake. Only a newer
  // inventory, from discovery or another refresh, supersedes it.
  async function refresh() {
    const initialInventory = useWorkbenchStore.getState().harnesses;
    setRefreshing(true);
    setError(null);
    try {
      const inventory = await getWorkbenchPorts().agent.listHarnesses();
      const current = useWorkbenchStore.getState();
      if (current.harnesses === initialInventory)
        await current.applyHarnessDiscovery(inventory);
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setRefreshing(false);
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
      {/* A block, so the button keeps its own width in the page column. */}
      <div>
        <button
          className="button button--compact"
          type="button"
          disabled={disabled}
          onClick={() => void refresh()}
        >
          {refreshing ? "Checking harnesses..." : "Refresh discovery"}
        </button>
      </div>
      {error ? <p role="alert">{error}</p> : null}
      <div className="agent-settings__card">
        {harnesses.slice(0, 32).map((harness) => {
          const availability = harnessAvailability(harness);
          const selected = harness.id === selectedId;
          const status = sentenceCase(
            `${availability.label}${harness.version ? ` · ${harness.version}` : ""}`,
          );
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
          // This page has the refresh button the chat picker points to.
          const reason =
            harness.installState === "notInstalled"
              ? "Install it, then choose Refresh discovery."
              : availability.disabledReason;
          return (
            <div key={harness.id}>
              <SettingsRow
                title={harness.name}
                description={`${status}. ${reason ?? signIn}`}
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
              {selected &&
              harness.accountModelDiscovery &&
              !availability.disabledReason ? (
                <CodexAccountSettings disabled={disabled || !projectId} />
              ) : null}
              {harness.signIn && !availability.disabledReason ? (
                <p>
                  {harness.signIn.command ? (
                    <>
                      Run <code>{harness.signIn.command}</code> in a
                      terminal.{" "}
                    </>
                  ) : null}
                  {harness.signIn.hint}
                </p>
              ) : null}
            </div>
          );
        })}
      </div>
      {!harnesses.length ? <p role="status">No harnesses discovered.</p> : null}
      <LazyModuleBoundary
        label="Your ACP agents"
        fallback={null}
        load={loadCustomAgentSettings}
        render={(CustomAgentSettings) => <CustomAgentSettings />}
      />
      <LazyHarnessNotice />
    </>
  );
}

function sentenceCase(text: string): string {
  return text.charAt(0).toUpperCase() + text.slice(1);
}
