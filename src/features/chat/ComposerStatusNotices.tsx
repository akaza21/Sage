import type { HarnessDiscoveryPhase } from "../../app/harnessSlice";
import { useWorkbenchStore } from "../../app/store";
import { harnessHasSignInCheck } from "../../domain/harness";
import type { ChatComposerRegionProps } from "./chatComposerRegionProps";
import { HarnessSignInPart } from "./lazyParts";

const EXPLAINED_PHASES: readonly HarnessDiscoveryPhase[] = [
  "checking",
  "signInRequired",
  "consent",
  "waiting",
  "error",
];

export function ComposerStatusNotices({
  workbenchError,
  chatError,
  signInHarness = null,
  authRecovery,
  imageStatus,
  missingBindingNotice,
  discoveryPhase,
  selectedHarnessId,
}: Pick<
  ChatComposerRegionProps,
  | "workbenchError"
  | "chatError"
  | "signInHarness"
  | "authRecovery"
  | "missingBindingNotice"
  | "discoveryPhase"
  | "selectedHarnessId"
> & { imageStatus: string | null }) {
  const harnesses = useWorkbenchStore((state) => state.harnesses);
  const observeSignIn =
    authRecovery ||
    signInHarness ||
    harnesses.some(
      (harness) =>
        harness.id === selectedHarnessId && harnessHasSignInCheck(harness),
    );
  const showMissingBinding =
    missingBindingNotice && !EXPLAINED_PHASES.includes(discoveryPhase);
  if (
    !observeSignIn &&
    !workbenchError &&
    !chatError &&
    !signInHarness &&
    !imageStatus &&
    !showMissingBinding
  )
    return null;
  return (
    <div
      className="composer__notices"
      role="region"
      aria-label="Composer notices"
      tabIndex={0}
    >
      {observeSignIn ? (
        <HarnessSignInPart
          part="ComposerHarnessSignInNotice"
          harnessId={selectedHarnessId}
          recovery={authRecovery}
          refusal={signInHarness}
          fallback={
            signInHarness && chatError ? (
              <div className="composer__error" role="alert">
                {chatError}
              </div>
            ) : null
          }
        />
      ) : null}
      {workbenchError ? (
        <div className="composer__error" role="alert">
          {workbenchError}
        </div>
      ) : null}
      {chatError && !signInHarness ? (
        <div className="composer__error" role="alert">
          {chatError}
        </div>
      ) : null}
      {imageStatus ? (
        <div className="composer__error" role="alert">
          {imageStatus}
        </div>
      ) : null}
      {showMissingBinding ? (
        <div className="composer__notice">
          No reviewed local binding is available for this harness.
        </div>
      ) : null}
    </div>
  );
}
