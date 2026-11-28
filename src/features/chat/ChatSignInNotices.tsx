import type { ReactNode } from "react";
import { useWorkbenchStore } from "../../app/store";
import type { ComposerStatusNoticeProps } from "./ComposerStatusNotices";
import { ComposerStatusMessages } from "./ComposerStatusMessages";
import { HarnessSignInPart } from "./lazyParts";
import { clearChatSignInError } from "./chatSignInRecovery";

export function HarnessAccountMenu(props: {
  avatar: ReactNode;
  onShowHarnesses?: () => void;
}) {
  return <HarnessSignInPart part="HarnessAccountMenu" {...props} />;
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

/** Command refusals have no turn yet; turn failures own their inline recovery. */
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
  const refusal = useWorkbenchStore(
    (state) => state.chatUiById[ownerChatId ?? ""],
  );
  const latestTurn = page?.turns.at(-1);
  const commandRefusal = errorKind === "signInRequired";
  const harness = harnesses.find(
    (entry) => entry.bindingId === refusal?.errorBindingId,
  );
  return (
    <ComposerStatusMessages
      {...notices}
      signInHarness={commandRefusal ? harness : null}
      authRecovery={
        commandRefusal && harness
          ? {
              harnessId: harness.id,
              receivedAt: commandRefusal ? refusal?.errorReceivedAt : null,
              scope: JSON.stringify([
                projectId,
                epoch,
                ownerChatId,
                latestTurn?.id,
              ]),
              onSignedIn: () => {
                clearChatSignInError(ownerChatId, refusal);
              },
            }
          : undefined
      }
    />
  );
}
