import { useEffect, useRef } from "react";
import type { AgentArtifactOwner } from "../domain/agentArtifact";
import { useAgentChangesState } from "./agentChangesState";

import { AgentChangesView, AgentChangesTab } from "./AgentChangesViews";

export function useAgentChangesPane(
  owner: AgentArtifactOwner | null,
  generation: number,
  expanded: boolean,
  onExpandedChange: ((expanded: boolean) => void) | undefined,
  projectId: string | null,
  chatId: string | null,
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

/**
 * Opening an otherwise empty pane shows the focused Chat's changes, as the
 * Codex desktop app does, instead of a waiting placeholder. Only the
 * transition to expanded opens them, so closing the Changes tab sticks.
 */
export function useChangesOnExpand(
  expanded: boolean,
  empty: boolean,
  owner: AgentArtifactOwner | null,
  generation: number,
) {
  const wasExpanded = useRef(expanded);
  useEffect(() => {
    const opened = expanded && !wasExpanded.current;
    wasExpanded.current = expanded;
    if (opened && empty && owner)
      useAgentChangesState.getState().show(owner, generation);
  }, [expanded, empty, owner, generation]);
}
