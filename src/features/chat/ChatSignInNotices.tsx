import { useWorkbenchStore } from "../../app/store";
import type { ComposerStatusNoticeProps } from "./ComposerStatusNotices";
import { ComposerStatusMessages } from "./ComposerStatusMessages";
import { HarnessSignInPart } from "../harness/harnessSignInLazy";
import { clearChatSignInError } from "./chatSignInRecovery";

export function HarnessAccountMenu({ avatar }: { avatar: string }) {
  return <HarnessSignInPart part="HarnessAccountMenu" avatar={avatar} />;
}

export function ComposerNoticeSurface({
  chatAuth,
  ...notices
}: ComposerStatusNoticeProps) {
  return chatAuth ? (
    <ChatSignInNotices {...notices} {...chatAuth} />
  ) : (
    <ComposerStatusMessages {...notices} />
  );
}

/** Resolve restored refusals only while the chat's notice surface is awake. */
function ChatSignInNotices({
  page,
  errorKind,
  boundChatId,
  ...notices
}: ComposerStatusNoticeProps &
  NonNullable<ComposerStatusNoticeProps["chatAuth"]>) {
  const harnesses = useWorkbenchStore((state) => state.harnesses);
  const projectId = useWorkbenchStore((state) => state.project?.id);
  const epoch = useWorkbenchStore((state) => state.nativeTransitionEpoch);
  const ownerChatId =
    boundChatId ?? page?.chatId ?? useWorkbenchStore.getState().activeTabId;
  const dismissedAuthTurnId = useWorkbenchStore(
    (state) => state.chatUiById[ownerChatId ?? ""]?.dismissedAuthTurnId,
  );
  const latestTurn = page?.turns.at(-1);
  const latestTurnSession = page?.sessions.find(
    (session) => session.id === latestTurn?.sessionId,
  );
  const signInHarness =
    errorKind === "signInRequired"
      ? harnesses.find((entry) => entry.id === notices.selectedHarnessId)
      : null;
  const harness =
    signInHarness ??
    harnesses.find((entry) => entry.bindingId === latestTurnSession?.bindingId);
  const authOwner = useWorkbenchStore
    .getState()
    .captureChatOwnership(ownerChatId ?? undefined);
  const signInRequired =
    errorKind === "signInRequired" ||
    (page?.chatId === ownerChatId &&
      dismissedAuthTurnId !== latestTurn?.id &&
      latestTurn?.state === "failed" &&
      latestTurn.terminalReason?.kind === "authRequired");
  return (
    <ComposerStatusMessages
      {...notices}
      signInHarness={
        signInHarness ?? (signInRequired && !notices.chatError ? harness : null)
      }
      authRecovery={
        signInRequired && harness
          ? {
              harnessId: harness.id,
              scope: JSON.stringify([
                projectId,
                epoch,
                ownerChatId,
                latestTurn?.id,
              ]),
              onSignedIn: () => {
                if (authOwner && latestTurn)
                  useWorkbenchStore
                    .getState()
                    .dismissChatAuthTurn(authOwner, latestTurn.id);
                if (errorKind === "signInRequired")
                  clearChatSignInError(boundChatId);
              },
            }
          : undefined
      }
    />
  );
}
