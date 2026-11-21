import { useWorkbenchStore } from "../../app/store";
import type { AcpChatPage, TurnProjection } from "../../domain/acp";
import {
  HarnessSignInRecovery,
  SignInRequiredMessage,
} from "./HarnessSignInStatus";

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
    state.chatUiById[page.chatId]?.dismissedAuthTurnId === turn.id ||
    state.chatUiById[page.chatId]?.errorKind === "signInRequired"
      ? undefined
      : state.harnesses.find((entry) => entry.bindingId === bindingId),
  );
  if (!harness) return null;
  const owner = useWorkbenchStore.getState().captureChatOwnership(page.chatId);
  return (
    <HarnessSignInRecovery
      refusal={
        <p>
          <SignInRequiredMessage harness={harness} />
        </p>
      }
      recovery={{
        harnessId: harness.id,
        scope: JSON.stringify([page.projectId, page.chatId, turn.id]),
        onSignedIn: () => {
          if (owner)
            useWorkbenchStore.getState().dismissChatAuthTurn(owner, turn.id);
        },
      }}
    />
  );
}
