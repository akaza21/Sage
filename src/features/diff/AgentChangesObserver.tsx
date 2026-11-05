import { useEffect, useMemo, useRef, useState } from "react";
import type { AcpChatPage } from "../../domain/acp";
import { isTauriRuntime } from "../../ports/desktopRuntime";
import { useTurnWorkspaceChanges } from "../chat/acp/turnWorkspaceBridge";
import { terminalTurnStates } from "../chat/acp/statusSemantics";
import { useAgentChangesState } from "../../workbench/agentChangesState";
import { changesSnapshot, snapshotChanges } from "./changesSnapshot";
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
  });
  if (transition.key !== key)
    setTransition({
      key,
      completed: !!currentTurn && terminalTurnStates.has(currentTurn.state),
    });
  const snapshot = useMemo(
    () =>
      changesSnapshot(
        transition.completed &&
          currentTurn?.sessionId === currentPage.currentSessionId
          ? currentPage
          : null,
      ),
    [currentPage, currentTurn?.sessionId, transition.completed],
  );
  const page = snapshot?.page ?? null;
  const ledger = useTurnWorkspaceChanges(page, isTauriRuntime());
  const handled = useRef("");
  const turn = page?.turns.at(-1);
  useEffect(() => {
    if (
      !page ||
      !turn ||
      turn.sessionId !== page.currentSessionId ||
      !terminalTurnStates.has(turn.state)
    )
      return;
    const key = turn.id;
    if (handled.current === key) return;
    const result = snapshotChanges(snapshot, ledger);
    if (!result.entries.length) return;
    handled.current = key;
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
  }, [generation, ledger, page, snapshot, turn]);
  return null;
}
