import { useEffect, useRef, useState } from "react";
import type { AcpChatPage } from "../../domain/acp";
import { isTauriRuntime } from "../../ports/desktopRuntime";
import { useTurnWorkspaceChanges } from "../chat/acp/turnWorkspaceBridge";
import { terminalTurnStates } from "../chat/acp/statusSemantics";
import { useAgentChangesState } from "../../workbench/agentChangesState";
import { projectChanges } from "./agentChanges";
import { acpProjectionStoreFor } from "../../app/acpProjectionRegistry";
import { agentArtifactOwnerIsCurrent } from "../../app/openAgentArtifact";
import {
  agentArtifactFollowRecordFor,
  useAgentArtifactFollowStore,
} from "../../app/agentArtifactFollowStore";

/** Follows completed edits once per Turn; explicit selections stay put. */
export function AgentChangesObserver({
  page: currentPage,
  initialPage,
  generation,
}: {
  page: AcpChatPage;
  initialPage: AcpChatPage;
  generation: number;
}) {
  const currentTurn = currentPage.turns.at(-1);
  const key = `${currentTurn?.id}:${currentTurn?.state}`;
  const [completion, setCompletion] = useState<{
    key: string;
    page: AcpChatPage | null;
  }>(() => {
    const initial = initialPage.turns.at(-1);
    return { key: `${initial?.id}:${initial?.state}`, page: null };
  });
  if (completion.key !== key)
    setCompletion({
      key,
      page:
        currentTurn && terminalTurnStates.has(currentTurn.state)
          ? currentPage
          : null,
    });
  const page = completion.page;
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
    const result = projectChanges(
      "lastTurn",
      page,
      null,
      ledger.changes,
      ledger.incompleteTurnIds,
    );
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
    if (
      agentArtifactFollowRecordFor(
        useAgentArtifactFollowStore.getState().records,
        owner,
      )?.mode === "pinned"
    )
      return;
    useAgentChangesState.getState().show(owner, generation);
  }, [generation, ledger, page, turn]);
  return null;
}
