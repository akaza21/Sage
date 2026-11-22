import { useState } from "react";
import { useWorkbenchStore } from "../../app/store";
import type { HarnessDiscoveryPhase } from "../../app/harnessSlice";
import {
  harnessUnavailableReason,
  preferredAvailableHarness,
  type HarnessDefinition,
} from "../../domain/harness";
import { Notice, type NoticeProps } from "../../ui/Notice";
import type {
  HarnessNoticePresentation,
  HarnessNoticePresentationProps,
} from "./harnessNoticePresentation";
import { useAgentSettingsState } from "../settings/agentSettingsState";
import "./build-notice.css";

/** Facts are shared; Settings retains its existing presentation until its phase. */
export function HarnessDiscoveryNotice({
  harness: provided,
  discoveryPhase,
  presentation: SettingsPresentation,
}: {
  harness?: HarnessDefinition | null;
  discoveryPhase?: HarnessDiscoveryPhase;
  presentation?: HarnessNoticePresentation;
} = {}) {
  const harnesses = useWorkbenchStore((s) => s.harnesses);
  const selectedId = useWorkbenchStore((s) => s.selectedHarnessId);
  const globalPhase = useWorkbenchStore((s) => s.harnessDiscoveryPhase);
  const error = useWorkbenchStore((s) => s.harnessDiscoveryError);
  const transitioning = useWorkbenchStore((s) => s.projectTransitioning);
  const {
    retryHarnessDiscovery: retry,
    confirmHarnessSignIn: confirm,
    cancelHarnessSignIn: cancel,
    openNewChatDraft,
  } = useWorkbenchStore.getState();
  const [dismissedBuild, dismissBuild] = useState("");
  const [dismissedRelease, dismissRelease] = useState("");
  const harness =
    provided === undefined
      ? harnesses.find((h) => h.id === selectedId)
      : provided;
  const phase = discoveryPhase ?? globalPhase;
  const present = (facts: Omit<HarnessNoticePresentationProps, "harness">) =>
    SettingsPresentation ? (
      <SettingsPresentation harness={harness} {...facts} />
    ) : (
      <>
        {[facts.binding, ...(facts.builds ?? []), facts.discovery].map(
          (notice) =>
            notice ? <Notice key={notice.identity} {...notice} /> : null,
        )}
        {facts.actions}
      </>
    );
  if (harness?.transport !== "acp" && (harness || SettingsPresentation))
    return null;
  const id = JSON.stringify([harness?.id, harness?.bindingId]);
  const update = harness?.update;
  if (!harness || harness.installState !== "available" || !harness.bindingId) {
    const pending = (harness ? [harness] : harnesses).some(
      (h) => h.installState === "detecting" || h.installState === "installing",
    );
    const message = harnessUnavailableReason(harness?.unavailableReason);
    const binding: NoticeProps = {
      identity: id,
      severity: pending ? "info" : "blocking",
      progress: pending,
      message: pending ? "Checking…" : harness ? message : "Choose a harness.",
      action: pending
        ? undefined
        : preferredAvailableHarness(harnesses)
          ? {
              label: "Continue in a new chat",
              onClick: openNewChatDraft,
            }
          : {
              label: "Open Settings",
              onClick: () => useAgentSettingsState.getState().open("Harnesses"),
            },
      details: update?.command,
    };
    return present({ binding });
  }
  const build = JSON.stringify([id, harness.boundVersion]);
  const release = JSON.stringify([id, update?.latestVersion]);
  const newer = harness.characterisation === "newerThanTested";
  const builds: NoticeProps[] = [];
  if (
    (newer || harness.characterisation === "uncharacterised") &&
    dismissedBuild !== build
  )
    builds.push({
      identity: build,
      severity: "warning",
      message: `${harness.name} ${harness.boundVersion ?? ""}${newer ? " is newer than tested; tested features stay on." : " hasn't been tested with Sage yet, so some features are off."}`,
      action: {
        label: "Dismiss build notice",
        onClick: () => dismissBuild(build),
      },
    });
  if (update && dismissedRelease !== release)
    builds.push({
      identity: release,
      severity: "info",
      message: `${harness.name} ${update.latestVersion} is available.`,
      details: update.command,
      action: {
        label: "Dismiss update",
        onClick: () => dismissRelease(release),
      },
    });
  const message =
    phase === "error"
      ? (error ?? "Harness discovery failed safely.")
      : !harness.accountModelDiscovery ||
          harness.characterisation === "uncharacterised"
        ? null
        : phase === "checking"
          ? `Checking the ${harness.name} account and available models…`
          : phase === "consent"
            ? `Sage will open this harness's official sign-in page in your browser. ${harness.name} stores credentials; Sage does not receive or retain them.`
            : phase === "waiting"
              ? "Finish signing in through your browser."
              : null;
  const actions =
    phase === "consent"
      ? ([
          ["Continue", confirm],
          ["Cancel", cancel],
        ] as const)
      : phase === "waiting"
        ? ([["Cancel", cancel]] as const)
        : phase === "error"
          ? ([["Retry", retry]] as const)
          : [];
  const buttons =
    message &&
    actions.map(([label, run]) => (
      <button
        key={label}
        className="button button--compact"
        type="button"
        disabled={transitioning}
        onClick={run}
      >
        {label}
      </button>
    ));
  const discovery: NoticeProps | undefined = message
    ? {
        identity: `${id}:${phase}`,
        severity: phase === "error" ? "blocking" : "info",
        progress: phase !== "error",
        message,
      }
    : undefined;
  return present({ builds, discovery, actions: buttons });
}
