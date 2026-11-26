import { useState } from "react";
import { useWorkbenchStore } from "../../app/store";
import type { AcpChatPage, AcpItem, TurnProjection } from "../../domain/acp";
import {
  HarnessSignInRecovery,
  SignInRequiredMessage,
} from "./HarnessSignInStatus";

export function HarnessTurnFailureDetails({
  turn,
  page,
  error,
}: {
  turn: TurnProjection;
  page?: AcpChatPage;
  error?: Extract<AcpItem, { kind: "error" }>["payload"];
}) {
  return (
    <>
      <details>
        <summary>Raw</summary>
        <pre>
          {JSON.stringify({ reason: turn.terminalReason, error }, null, 2)}
        </pre>
      </details>
      {turn.terminalReason?.kind === "authRequired" && page ? (
        <HarnessTurnSignInRecovery page={page} turn={turn} />
      ) : null}
    </>
  );
}

// Disposable WebView state: the newest 64 refused turns a later sign-in answered.
const answered: string[] = [];

function HarnessTurnSignInRecovery({
  page,
  turn,
}: {
  page: AcpChatPage;
  turn: TurnProjection;
}) {
  const bindingId = page.sessions.find(
    (session) => session.id === turn.sessionId,
  )?.bindingId;
  const [done, setDone] = useState(() => answered.includes(turn.id));
  const harness = useWorkbenchStore((state) =>
    done || state.chatUiById[page.chatId]?.errorKind === "signInRequired"
      ? undefined
      : state.harnesses.find((entry) => entry.bindingId === bindingId),
  );
  if (!harness) return null;
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
          if (answered.push(turn.id) > 64) answered.shift();
          setDone(true);
        },
      }}
    />
  );
}
