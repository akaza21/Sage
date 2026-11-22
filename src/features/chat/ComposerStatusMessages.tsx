import { HarnessSignInPart } from "./lazyParts";
import { Notice } from "../../ui/Notice";
import {
  ComposerNoticeFrame,
  type ComposerStatusNoticeProps,
} from "./ComposerStatusNotices";

export function ComposerStatusMessages({
  chatError,
  signInHarness = null,
  authRecovery,
  ...props
}: ComposerStatusNoticeProps) {
  return (
    <ComposerNoticeFrame
      {...props}
      chatError={signInHarness ? null : chatError}
    >
      {authRecovery ? (
        <HarnessSignInPart
          part="HarnessSignInRecovery"
          recovery={authRecovery}
          refusal={
            signInHarness ? (
              <Notice
                identity={`sign-in:${authRecovery.scope}`}
                severity="blocking"
                message={
                  <HarnessSignInPart
                    part="SignInRequiredMessage"
                    harness={signInHarness}
                  />
                }
              />
            ) : null
          }
          fallback={
            signInHarness && chatError ? (
              <Notice
                identity={`sign-in:${authRecovery.scope}`}
                severity="blocking"
                message={chatError.message}
              />
            ) : null
          }
        />
      ) : null}
    </ComposerNoticeFrame>
  );
}
