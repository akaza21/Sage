import { useEffect, useLayoutEffect, useRef } from "react";
import type { AgentArtifactOwner } from "../domain/agentArtifact";
import { useAgentChangesState, type ChangesOwner } from "./agentChangesState";

import { AgentChangesView, AgentChangesTab } from "./AgentChangesViews";

/**
 * A Chat whose loaded projection confirms it has no harness session yet still
 * owns a (possibly empty) review, as in the Codex desktop app, so Changes can
 * open before a session starts. While the projection is loading or failed, a
 * Chat's session is unknown, so it gets no owner and keeps its waiting state.
 * A sessionless Changes tab stays with its Chat once a session starts.
 */
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
  /** The Chat's loaded projection confirms it has no session. */
  sessionless = false,
) {
  const state = useAgentChangesState();
  const sameChat =
    !!state.owner &&
    state.owner.projectId === projectId &&
    state.owner.chatId === chatId &&
    state.generation === generation;
  // A sessionless tab needs a validated projection: one confirming there is
  // still no session, or one naming the session the Chat has since started,
  // which the layout effect below then adopts as the tab's owner.
  const open =
    sameChat &&
    (state.owner!.sessionId === null
      ? sessionless || owner !== null
      : state.owner!.sessionId === owner?.sessionId);
  const selected = open && state.selected;
  useLayoutEffect(() => {
    if (open && owner && state.owner?.sessionId === null) state.adopt(owner);
  }, [open, owner, state]);
  const previousOwner = useRef(state.owner);
  useEffect(() => {
    const previous = previousOwner.current;
    previousOwner.current = state.owner;
    // Adopting the started session is the same tab, not a new request.
    const adopted =
      previous?.sessionId === null && previous.chatId === state.owner?.chatId;
    if (selected && !expanded && previous !== state.owner && !adopted)
      onExpandedChange?.(true);
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
    owner: changesOwnerFor(owner, projectId, chatId, sessionless),
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
