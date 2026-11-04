import { useEffect, useRef } from "react";
import type { AgentArtifactOwner } from "../domain/agentArtifact";
import type { AcpChatPage } from "../domain/acp";
import { useAgentChangesState } from "./agentChangesState";

import {
  AgentChangesView,
  AgentChangesTab,
  AgentChangesObserver,
} from "./AgentChangesViews";

export function useAgentChangesPane(
  owner: AgentArtifactOwner | null,
  generation: number,
  expanded: boolean,
  onExpandedChange: ((expanded: boolean) => void) | undefined,
  projectId: string | null,
  chatId: string | null,
  page: AcpChatPage | null = null,
) {
  const state = useAgentChangesState();
  const open =
    !!state.owner &&
    state.owner.projectId === projectId &&
    state.owner.chatId === chatId &&
    state.owner.sessionId === (owner?.sessionId ?? null) &&
    state.generation === generation;
  const selected = open && state.selected;
  const previousOwner = useRef(state.owner);
  useEffect(() => {
    const changed = previousOwner.current !== state.owner;
    previousOwner.current = state.owner;
    if (selected && !expanded && changed) onExpandedChange?.(true);
  }, [selected, expanded, onExpandedChange, state.owner]);
  return {
    observer: (
      <AgentChangesObserver
        key={JSON.stringify([generation, projectId, chatId, owner?.sessionId])}
        page={page}
        generation={generation}
      />
    ),
    open,
    selected,
    tab: open ? (
      <AgentChangesTab
        selected={selected}
        select={() => state.select(true)}
        close={state.close}
      />
    ) : null,
    surface: selected ? (
      <AgentChangesView
        key={`${generation}:${chatId}:${owner?.sessionId}`}
        chatId={owner?.chatId ?? null}
      />
    ) : null,
    select: () => state.select(true),
    close: state.close,
  };
}
