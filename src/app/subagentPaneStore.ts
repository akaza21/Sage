import { create } from "zustand";

import type { AgentArtifactOwner } from "../domain/agentArtifact";
import type { AcpChatPage } from "../domain/acp";
import {
  sameAgentArtifactOwner,
  validAgentArtifactOwner,
} from "../domain/agentArtifact";

export type SubagentPaneTarget = {
  owner: AgentArtifactOwner;
  itemId: string;
};

export function validFocusedSubagentPage(
  page: AcpChatPage | null,
  scope: {
    projectId: string;
    chatId: string;
    projectGeneration: string;
  } | null,
  projectId: string | null,
  chatId: string | null,
  generation: number,
  transitioning: boolean,
): boolean {
  return Boolean(
    !transitioning &&
    projectId &&
    chatId &&
    page &&
    scope &&
    page.projectId === projectId &&
    page.chatId === chatId &&
    scope.projectId === page.projectId &&
    scope.chatId === page.chatId &&
    scope.projectGeneration === String(generation),
  );
}

export function ownerFromFocusedPage(
  page: AcpChatPage | null,
  valid: boolean,
): AgentArtifactOwner | null {
  return valid && page?.currentSessionId
    ? {
        projectId: page.projectId,
        chatId: page.chatId,
        sessionId: page.currentSessionId,
      }
    : null;
}

interface SubagentPaneStore {
  open: SubagentPaneTarget | null;
  selected: boolean;
  show: (target: SubagentPaneTarget) => void;
  select: (selected: boolean) => void;
  clear: () => void;
}

export function subagentPaneForPage(
  target: SubagentPaneTarget | null,
  page: AcpChatPage | null,
): SubagentPaneTarget | null {
  if (
    !target ||
    !page ||
    target.owner.projectId !== page.projectId ||
    target.owner.chatId !== page.chatId
  )
    return null;
  const found = page.turns.some(
    (turn) =>
      turn.sessionId === target.owner.sessionId &&
      turn.items.some(
        (item) =>
          item.id === target.itemId &&
          item.kind === "collaboration" &&
          sameAgentArtifactOwner(target.owner, item),
      ),
  );
  return found ? target : null;
}

export const useSubagentPaneStore = create<SubagentPaneStore>((set) => ({
  open: null,
  selected: false,
  show: (target) => {
    if (!validAgentArtifactOwner(target.owner) || !target.itemId) return;
    set({ open: target, selected: true });
  },
  select: (selected) => set({ selected }),
  clear: () => set({ open: null, selected: false }),
}));
