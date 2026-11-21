import { HarnessSignInPart } from "./lazyParts";
import {
  ComposerNoticeFrame,
  type ComposerStatusNoticeProps,
} from "./ComposerStatusNotices";

export function ComposerStatusMessages({
  chatError,
  signInHarness = null,
  authRecovery,
  selectedHarnessId,
  ...props
}: ComposerStatusNoticeProps) {
  return (
    <ComposerNoticeFrame
      {...props}
      selectedHarnessId={selectedHarnessId}
      chatError={signInHarness ? null : chatError}
    >
      {authRecovery ? (
        <HarnessSignInPart
          part="HarnessSignInRecovery"
          recovery={authRecovery}
          refusal={
            signInHarness ? (
              <div className="composer__error" role="alert">
                <HarnessSignInPart
                  part="SignInRequiredMessage"
                  harness={signInHarness}
                />
              </div>
            ) : null
          }
          fallback={
            signInHarness && chatError ? (
              <div className="composer__error" role="alert">
                {chatError}
              </div>
            ) : null
          }
        />
      ) : null}
    </ComposerNoticeFrame>
  );
}
