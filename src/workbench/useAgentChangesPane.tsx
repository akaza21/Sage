import { useEffect, useLayoutEffect, useRef } from "react";
import type { AgentArtifactOwner } from "../domain/agentArtifact";
import { useAgentChangesState, type ChangesOwner } from "./agentChangesState";

import { AgentChangesView, AgentChangesTab } from "./AgentChangesViews";

/** Only a Chat whose loaded projection confirms it has no session yet owns a
 * sessionless review. */
export function changesOwnerFor(
  owner: AgentArtifactOwner | null,
  projectId: string | null,
  chatId: string | null,
  sessionless: boolean,
): ChangesOwner | null {
  if (owner) return owner;
  return sessionless && projectId && chatId
    ? { projectId, chatId, sessionId: null }
    : null;
}

export function useAgentChangesPane(
  owner: AgentArtifactOwner | null,
  generation: number,
  expanded: boolean,
  onExpandedChange: ((expanded: boolean) => void) | undefined,
  projectId: string | null,
  chatId: string | null,
  sessionless = false,
) {
  const state = useAgentChangesState();
  const sameChat =
    !!state.owner &&
    state.owner.projectId === projectId &&
    state.owner.chatId === chatId &&
    state.generation === generation;
  const open =
    sameChat &&
    (state.owner!.sessionId === null
      ? sessionless || owner !== null
      : state.owner!.sessionId === owner?.sessionId);
  const selected = open && state.selected;
  useLayoutEffect(() => {
    if (open && owner && state.owner?.sessionId === null) state.adopt(owner);
  }, [open, owner, state]);
  const handledRequest = useRef(state.request);
  useEffect(() => {
    if (handledRequest.current === state.request) return;
    const forThisChat = state.selected && sameChat;
    if (forThisChat && !selected) return;
    handledRequest.current = state.request;
    if (forThisChat && !expanded) onExpandedChange?.(true);
  }, [
    expanded,
    onExpandedChange,
    sameChat,
    selected,
    state.request,
    state.selected,
  ]);
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
    owner: changesOwnerFor(owner, projectId, chatId, sessionless),
  };
}

/** Only the transition to expanded opens the Chat's changes, so closing the
 * Changes tab sticks. */
export function useChangesOnExpand(
  expanded: boolean,
  empty: boolean,
  owner: ChangesOwner | null,
  generation: number,
) {
  const wasExpanded = useRef(expanded);
  // Survives until the owner arrives; collapsing cancels it.
  const pending = useRef(false);
  useEffect(() => {
    if (expanded !== wasExpanded.current) pending.current = expanded;
    wasExpanded.current = expanded;
    if (!pending.current || !owner) return;
    pending.current = false;
    if (empty) useAgentChangesState.getState().show(owner, generation);
  }, [expanded, empty, owner, generation]);
}
