import { useWorkbenchStore } from "../../app/store";
import { harnessHasSignInCheck } from "../../domain/harness";
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
  const harnesses = useWorkbenchStore((state) => state.harnesses);
  const observeSignIn =
    authRecovery ||
    signInHarness ||
    harnesses.some(
      (harness) =>
        harness.id === selectedHarnessId && harnessHasSignInCheck(harness),
    );
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
