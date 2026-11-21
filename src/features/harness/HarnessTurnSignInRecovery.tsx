import { useWorkbenchStore } from "../../app/store";
import type { AcpChatPage, TurnProjection } from "../../domain/acp";
import { HarnessSignInPart } from "./harnessSignInLazy";

export function HarnessTurnSignInRecovery({
  page,
  turn,
}: {
  page: AcpChatPage;
  turn: TurnProjection;
}) {
  const bindingId = page.sessions.find(
    (session) => session.id === turn.sessionId,
  )?.bindingId;
  const harness = useWorkbenchStore((state) =>
    state.harnesses.find((entry) => entry.bindingId === bindingId),
  );
  const dismissed = useWorkbenchStore(
    (state) =>
      state.chatUiById[page.chatId]?.dismissedAuthTurnId === turn.id ||
      state.chatUiById[page.chatId]?.errorKind === "signInRequired",
  );
  const owner = useWorkbenchStore.getState().captureChatOwnership(page.chatId);
  return harness && !dismissed ? (
    <HarnessSignInPart
      part="ComposerHarnessSignInNotice"
      harnessId={harness.id}
      inline
      recovery={{
        harnessId: harness.id,
        scope: JSON.stringify([page.projectId, page.chatId, turn.id]),
        onSignedIn: () => {
          if (owner)
            useWorkbenchStore.getState().dismissChatAuthTurn(owner, turn.id);
        },
      }}
    />
  ) : null;
}
