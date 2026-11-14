import { memo, useState } from "react";
import { useWorkbenchStore } from "../../app/store";
import { Info, X } from "../../ui/leanIcons";
import type { HarnessDiscoveryPhase } from "../../app/harnessSlice";
import {
  harnessUnavailableReason,
  type HarnessDefinition,
} from "../../domain/harness";
import "./build-notice.css";

const BoundBuildNotice = memo(function BoundBuildNotice({
  harness,
}: {
  harness: HarnessDefinition | null;
}) {
  const [dismissed, setDismissed] = useState<readonly string[]>([]);
  const update = harness?.update;
  const build = JSON.stringify([
    harness?.id,
    harness?.bindingId,
    harness?.boundVersion,
  ]);
  const release = JSON.stringify([harness?.id, update?.latestVersion]);
  if (harness?.transport !== "acp") return null;
  const howToUpdate = update ? (
    <>
      Run <code>{update.command}</code>, then refresh discovery in Settings.
    </>
  ) : (
    "Update it, then refresh harness discovery in Settings."
  );
  if (harness.installState !== "available") {
    return (
      <div className="composer__notice" role="status">
        {harness.unavailableReason === "versionTooOld" ? (
          <>This version is too old for Sage. {howToUpdate}</>
        ) : (
          harnessUnavailableReason(harness.unavailableReason)
        )}
      </div>
    );
  }
  const newer = harness.characterisation === "newerThanTested";
  const showBuild =
    (newer || harness.characterisation === "uncharacterised") &&
    !dismissed.includes(build);
  const showUpdate = update && !dismissed.includes(release);
  if (!showBuild && !showUpdate) return null;
  return (
    <div className="build-notice" role="status">
      <Info aria-hidden="true" />
      <span>
        {showBuild ? (
          <>
            {harness.name} {harness.boundVersion}
            {newer
              ? " is newer than Sage has tested. Tested features stay on. "
              : " hasn't been tested with Sage yet, so some features are off. "}
          </>
        ) : null}
        {showUpdate ? (
          <>
            {harness.name} {update.latestVersion} is available. {howToUpdate}
          </>
        ) : null}
      </span>
      <button
        type="button"
        aria-label="Dismiss build notice"
        title="Dismiss"
        onClick={() => setDismissed([build, release])}
      >
        <X aria-hidden="true" />
      </button>
    </div>
  );
});

function AccountNotice({
  harness,
  phase,
}: {
  harness: HarnessDefinition | null;
  phase: HarnessDiscoveryPhase;
}) {
  const error = useWorkbenchStore((state) => state.harnessDiscoveryError);
  const transitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const retry = useWorkbenchStore((state) => state.retryHarnessDiscovery);
  const requestSignIn = useWorkbenchStore(
    (state) => state.requestHarnessSignIn,
  );
  const confirmSignIn = useWorkbenchStore(
    (state) => state.confirmHarnessSignIn,
  );
  const cancelSignIn = useWorkbenchStore((state) => state.cancelHarnessSignIn);
  if (harness?.transport !== "acp" || !harness.accountModelDiscovery)
    return null;
  if (harness.installState !== "available") return null;
  if (harness.characterisation === "uncharacterised") return null;
  const harnessName = harness.name;

  if (phase === "idle") {
    return (
      <div className="composer__notice" role="status">
        Sign-in status for {harnessName} is unknown. Start a conversation to
        check whether sign-in is required.
      </div>
    );
  }
  if (phase === "checking") {
    return (
      <div className="composer__notice" role="status">
        Checking the {harnessName} account and available models…
      </div>
    );
  }
  if (phase === "signInRequired") {
    return (
      <div className="composer__notice" role="status">
        Sign in to the account required by {harnessName} to load models.
        <button
          className="button button--compact"
          type="button"
          disabled={transitioning}
          onClick={requestSignIn}
        >
          Sign in
        </button>
      </div>
    );
  }
  if (phase === "consent") {
    return (
      <div className="composer__notice" role="status">
        Sage will open this harness's official sign-in page in your browser.{" "}
        {harnessName} stores credentials; Sage does not receive or retain them.
        <button
          className="button button--compact"
          type="button"
          disabled={transitioning}
          onClick={() => void confirmSignIn()}
        >
          Continue
        </button>
        <button
          className="button button--compact"
          type="button"
          disabled={transitioning}
          onClick={() => void cancelSignIn()}
        >
          Cancel
        </button>
      </div>
    );
  }
  if (phase === "waiting") {
    return (
      <div className="composer__notice" role="status">
        Finish signing in through your browser.
        <button
          className="button button--compact"
          type="button"
          disabled={transitioning}
          onClick={() => void cancelSignIn()}
        >
          Cancel
        </button>
      </div>
    );
  }
  if (phase === "ready") return null;
  if (phase === "error") {
    return (
      <div className="composer__error" role="alert">
        {error ?? "Harness discovery failed safely."}
        <button
          className="button button--compact"
          type="button"
          disabled={transitioning}
          onClick={() => void retry()}
        >
          Retry
        </button>
      </div>
    );
  }
  return null;
}

export function HarnessDiscoveryNotice({
  harness: providedHarness,
  discoveryPhase: providedDiscoveryPhase,
}: {
  harness?: HarnessDefinition | null;
  discoveryPhase?: HarnessDiscoveryPhase;
} = {}) {
  const selectedHarness = useWorkbenchStore((state) => {
    const selected = state.selectedHarnessId;
    return (
      state.harnesses.find((candidate) => candidate.id === selected) ?? null
    );
  });
  const globalDiscoveryPhase = useWorkbenchStore(
    (state) => state.harnessDiscoveryPhase,
  );
  const harness =
    providedHarness === undefined ? selectedHarness : providedHarness;
  const discoveryPhase = providedDiscoveryPhase ?? globalDiscoveryPhase;
  return (
    <>
      <BoundBuildNotice harness={harness} />
      <AccountNotice harness={harness} phase={discoveryPhase} />
    </>
  );
}
