import { useEffect, useRef } from "react";
import type { AgentArtifactOwner } from "../domain/agentArtifact";
import { useAgentChangesState, type ChangesOwner } from "./agentChangesState";

import { AgentChangesView, AgentChangesTab } from "./AgentChangesViews";

/**
 * A Chat with no harness session yet still owns a (possibly empty) review, as
 * in the Codex desktop app, so Changes can open before a session starts. A
 * sessionless Changes tab stays with its Chat once a session begins.
 */
export function changesOwnerFor(
  owner: AgentArtifactOwner | null,
  projectId: string | null,
  chatId: string | null,
): ChangesOwner | null {
  if (owner) return owner;
  return projectId && chatId ? { projectId, chatId, sessionId: null } : null;
}

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
    (state.owner.sessionId === null ||
      state.owner.sessionId === (owner?.sessionId ?? null)) &&
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
        chatId={chatId}
      />
    ) : null,
    select: () => state.select(true),
    close: state.close,
    owner: changesOwnerFor(owner, projectId, chatId),
  };
}

/**
 * Opening an otherwise empty pane shows the focused Chat's changes, as the
 * Codex desktop app does, instead of a waiting placeholder. Only the
 * transition to expanded opens them, so closing the Changes tab sticks. A
 * pane opened before the Chat's owner loads keeps that request until the
 * owner arrives or the pane closes.
 */
export function useChangesOnExpand(
  expanded: boolean,
  empty: boolean,
  owner: ChangesOwner | null,
  generation: number,
) {
  const wasExpanded = useRef(expanded);
  const pending = useRef(false);
  useEffect(() => {
    if (expanded !== wasExpanded.current) pending.current = expanded;
    wasExpanded.current = expanded;
    if (!pending.current || !owner) return;
    pending.current = false;
    if (empty) useAgentChangesState.getState().show(owner, generation);
  }, [expanded, empty, owner, generation]);
}
