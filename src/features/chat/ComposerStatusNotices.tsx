import type { HarnessDiscoveryPhase } from "../../app/harnessSlice";
import type { ChatComposerRegionProps } from "./chatComposerRegionProps";
import type * as ClaudeAuthChunk from "./ClaudeAuthActions";
import { lazyParts } from "./lazyParts";

const AuthPart = lazyParts<typeof ClaudeAuthChunk>(
  () => import("./ClaudeAuthActions"),
);

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
  claudeSignInRequired,
  claudeAuthScope,
  imageStatus,
  missingBindingNotice,
  discoveryPhase,
}: Pick<
  ChatComposerRegionProps,
  | "workbenchError"
  | "chatError"
  | "signInHarness"
  | "claudeSignInRequired"
  | "claudeAuthScope"
  | "missingBindingNotice"
  | "discoveryPhase"
> & { imageStatus: string | null }) {
  return (
    <>
      {workbenchError ? (
        <div className="composer__error" role="alert">
          {workbenchError}
        </div>
      ) : null}
      {chatError || signInHarness ? (
        <div className="composer__error" role="alert">
          {signInHarness ? (
            <AuthPart
              part="SignInRequiredMessage"
              harness={signInHarness}
              fallback={chatError}
            />
          ) : (
            chatError
          )}
          {claudeSignInRequired ? (
            <AuthPart part="ClaudeAuthActions" key={claudeAuthScope} />
          ) : null}
        </div>
      ) : null}
      {imageStatus ? (
        <div className="composer__error" role="alert">
          {imageStatus}
        </div>
      ) : null}
      {missingBindingNotice && !EXPLAINED_PHASES.includes(discoveryPhase) ? (
        <div className="composer__notice">
          No reviewed local binding is available for this harness.
        </div>
      ) : null}
    </>
  );
}
