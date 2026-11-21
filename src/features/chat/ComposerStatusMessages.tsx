import { HarnessSignInPart } from "../harness/harnessSignInLazy";
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
  const observeSignIn = authRecovery || signInHarness;
  return (
    <ComposerNoticeFrame
      {...props}
      selectedHarnessId={selectedHarnessId}
      chatError={signInHarness ? null : chatError}
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
    </ComposerNoticeFrame>
  );
}
