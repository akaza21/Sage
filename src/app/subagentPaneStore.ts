import { create } from "zustand";

import type { AgentArtifactOwner } from "../domain/agentArtifact";
import type { AcpChatPage, AcpItem } from "../domain/acp";
import {
  sameAgentArtifactOwner,
  validAgentArtifactOwner,
} from "../domain/agentArtifact";

export type SubagentPaneTarget = {
  owner: AgentArtifactOwner;
  itemId: string;
  agentKey: string;
};

export function attributedSubagentKey(
  item: Extract<AcpItem, { kind: "collaboration" }>,
  candidates: readonly (string | null | undefined)[],
): string | null {
  if (candidates.length !== 1 || (item.payload.agentKeys?.length ?? 0) > 1)
    return null;
  const key = candidates[0];
  if (
    item.payload.agentKey &&
    item.payload.agentKeys?.length === 1 &&
    item.payload.agentKey !== item.payload.agentKeys[0]
  )
    return null;
  const state = item.payload.agentStates?.find(([id]) => id === key)?.[1];
  if (state === "errored" || state === "notFound") return null;
  return key &&
    (item.payload.agentKey === key ||
      (item.payload.agentKeys?.length === 1 &&
        item.payload.agentKeys[0] === key))
    ? key
    : null;
}

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
          sameAgentArtifactOwner(target.owner, item) &&
          attributedSubagentKey(item, [target.agentKey]) === target.agentKey,
      ),
  );
  return found ? target : null;
}

export function latestSubagentForPage(
  page: AcpChatPage | null,
): SubagentPaneTarget | null {
  if (!page) return null;
  let remaining = 256;
  for (const turn of page.turns.slice(-128).reverse()) {
    for (const item of turn.items.slice(-remaining).reverse()) {
      remaining -= 1;
      if (item.kind !== "collaboration") continue;
      const keys = item.payload.agentKeys ?? [];
      const agentKey = attributedSubagentKey(item, [
        keys.length === 1 ? keys[0] : item.payload.agentKey,
      ]);
      if (!agentKey) continue;
      return {
        owner: {
          projectId: page.projectId,
          chatId: page.chatId,
          sessionId: turn.sessionId,
        },
        itemId: item.id,
        agentKey,
      };
    }
    if (remaining <= 0) break;
  }
  return null;
}

export const useSubagentPaneStore = create<SubagentPaneStore>((set) => ({
  open: null,
  selected: false,
  show: (target) => {
    if (
      !validAgentArtifactOwner(target.owner) ||
      !target.itemId ||
      !target.agentKey
    )
      return;
    set({ open: { ...target }, selected: true });
  },
  select: (selected) => set({ selected }),
  clear: () => set({ open: null, selected: false }),
}));
