import { useEffect, useState, useSyncExternalStore } from "react";
import { useWorkbenchStore } from "../../app/store";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import { isTauriRuntime } from "../../ports/desktopRuntime";
import { errorMessage } from "../../domain/errors";
import { harnessAvailability } from "../chat/harnessAvailability";
import { LazyHarnessNotice } from "../chat/lazyParts";
import {
  ensureHarnessSignInChecked,
  harnessSignInRevision,
  harnessSignInStatus,
  subscribeHarnessSignIn,
} from "../harness/harnessSignIn";
import { LazyModuleBoundary } from "../../ui/LazyModuleBoundary";
import { HarnessCard } from "./HarnessCard";
import { harnessCardStatus, harnessGroupRank } from "./harnessCardStatus";
import { HarnessUpdateCheckSetting } from "./HarnessVersions";
import type { HarnessNoticePresentation } from "../chat/harnessNoticePresentation";

// The user-defined agent editor loads only when Harnesses is open, below
// the harness list, so it arrives without a loading notice.
const loadCustomAgentSettings = () => import("./CustomAgentSettings");

export function AgentHarnessSettings({
  presentation,
}: {
  presentation: HarnessNoticePresentation;
}) {
  const harnesses = useWorkbenchStore((state) => state.harnesses);
  const selectedId = useWorkbenchStore((state) => state.selectedHarnessId);
  const phase = useWorkbenchStore((state) => state.harnessDiscoveryPhase);
  const transitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const projectId = useWorkbenchStore((state) => state.project?.id);
  const selectHarness = useWorkbenchStore((state) => state.selectHarness);
  useSyncExternalStore(subscribeHarnessSignIn, harnessSignInRevision);
  useEffect(() => {
    if (isTauriRuntime())
      ensureHarnessSignInChecked(harnesses.slice(0, 32).map(({ id }) => id));
  }, [harnesses]);
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
      const inventory = await getWorkbenchPorts().agent.refreshHarnesses();
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
  const cards = harnesses
    .slice(0, 32)
    // A Chat bound to a harness this build lacks keeps its name for that
    // Chat; it is not a harness on this computer, so Settings omits it.
    .filter((harness) => harness.unavailableReason !== "bindingMissing")
    .map((harness) => {
      const availability = harnessAvailability(harness);
      return {
        harness,
        note:
          harness.installState === "available" &&
          availability.label !== "available"
            ? sentenceCase(availability.label.replace(/^available — /, ""))
            : null,
        status: harnessCardStatus({
          harness,
          disabledReason: availability.disabledReason,
          selected: harness.id === selectedId,
          phase,
          probe: harnessSignInStatus(harness.id).state,
        }),
      };
    });
  const groups = [
    { label: "Available harnesses", rank: 0 },
    { label: "Unavailable harnesses", rank: 1 },
    { label: "Harnesses not installed", rank: 2 },
  ].map((group) => ({
    label: group.label,
    cards: cards.filter(
      (card) => harnessGroupRank(card.status.badge) === group.rank,
    ),
  }));
  return (
    <>
      <p>
        Sage finds the harnesses installed on this computer. Install a harness
        outside Sage, then refresh discovery.
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
      <HarnessUpdateCheckSetting />
      {groups.map((group) =>
        group.cards.length ? (
          <div
            key={group.label}
            className="harness-group"
            role="group"
            aria-label={group.label}
          >
            <p className="harness-group__label" aria-hidden="true">
              {group.label}
            </p>
            {group.cards.map(({ harness, status, note }) => (
              <HarnessCard
                key={harness.id}
                harness={harness}
                status={status}
                note={note}
                selected={harness.id === selectedId}
                actionDisabled={disabled || !projectId}
                detailsDisabled={disabled || !projectId}
                onSelect={() => void selectHarness(harness.id)}
              />
            ))}
          </div>
        ) : null,
      )}
      {!harnesses.length ? <p role="status">No harnesses discovered.</p> : null}
      <LazyModuleBoundary
        label="Your ACP agents"
        fallback={null}
        load={loadCustomAgentSettings}
        render={(CustomAgentSettings) => <CustomAgentSettings />}
      />
      <LazyHarnessNotice presentation={presentation} />
    </>
  );
}

function sentenceCase(text: string): string {
  return text.charAt(0).toUpperCase() + text.slice(1);
}
