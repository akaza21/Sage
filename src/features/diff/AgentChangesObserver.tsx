import { useCallback, useEffect, useMemo, useState } from "react";
import type { AcpChatPage } from "../../domain/acp";
import { isTauriRuntime } from "../../ports/desktopRuntime";
import { terminalTurnStates } from "../chat/acp/statusSemantics";
import { useAgentChangesState } from "../../workbench/agentChangesState";
import { changesSnapshot, snapshotChanges } from "./changesSnapshot";
import { useTurnWorkspaceChanges } from "../chat/acp/turnWorkspaceBridge";
import { acpProjectionStoreFor } from "../../app/acpProjectionRegistry";
import { agentArtifactOwnerIsCurrent } from "../../app/openAgentArtifact";

/** Follows completed edits once per Turn; explicit selections stay put. */
export function AgentChangesObserver({
  page: currentPage,
  initialTurnKey,
  generation,
}: {
  page: AcpChatPage;
  initialTurnKey: string;
  generation: number;
}) {
  const currentTurn = currentPage.turns.at(-1);
  const key = `${currentTurn?.id}:${currentTurn?.state}`;
  const [transition, setTransition] = useState({
    key: initialTurnKey,
    completed: false,
    handledTurnId: "",
  });
  if (transition.key !== key)
    setTransition({
      ...transition,
      key,
      completed:
        !!currentTurn &&
        terminalTurnStates.has(currentTurn.state) &&
        transition.handledTurnId !== currentTurn.id,
    });
  const handleCompletion = useCallback(
    (turnId: string) => {
      setTransition((current) =>
        current.key === key
          ? { key, completed: false, handledTurnId: turnId }
          : current,
      );
    },
    [key, setTransition],
  );
  return transition.completed &&
    currentTurn?.sessionId === currentPage.currentSessionId ? (
    <CompletedChanges
      key={key}
      page={currentPage}
      generation={generation}
      onHandled={handleCompletion}
    />
  ) : null;
}

/** Unmounting discards both React hook chains, including the previous render. */
function CompletedChanges({
  page: currentPage,
  generation,
  onHandled,
}: {
  page: AcpChatPage;
  generation: number;
  onHandled: (turnId: string) => void;
}) {
  const snapshot = useMemo(() => changesSnapshot(currentPage), [currentPage]);
  const page = snapshot?.page ?? null;
  const ledger = useTurnWorkspaceChanges(page, isTauriRuntime());
  const turn = page?.turns.at(-1);
  useEffect(() => {
    if (
      !page ||
      !turn ||
      turn.sessionId !== page.currentSessionId ||
      !terminalTurnStates.has(turn.state)
    )
      return;
    const result = snapshotChanges(snapshot, ledger);
    if (!result.entries.length) return;
    onHandled(turn.id);
    const owner = {
      projectId: page.projectId,
      chatId: page.chatId,
      sessionId: turn.sessionId,
    };
    const latest = acpProjectionStoreFor(page.chatId)
      .getState()
      .page?.turns.at(-1);
    if (
      !agentArtifactOwnerIsCurrent(owner, generation) ||
      latest?.id !== turn.id ||
      !terminalTurnStates.has(latest.state)
    )
      return;
    useAgentChangesState.getState().show(owner, generation, "follow");
  }, [generation, ledger, onHandled, page, snapshot, turn]);
  return null;
}
