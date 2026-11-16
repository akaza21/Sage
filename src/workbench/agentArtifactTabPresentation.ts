import type { AcpChatPage } from "../domain/acpProjection";
import type { AgentArtifactOwner } from "../domain/agentArtifact";
import {
  attributedSubagentKey,
  type SubagentPaneTarget,
} from "../app/subagentPaneStore";
import { sameAgentArtifactOwner } from "../domain/agentArtifact";
import type { AcpItem, TurnProjection } from "../domain/acp";

type ArtifactTab = { resource?: { relativePath?: string }; title: string };

const MAX_RESOURCE_TURNS = 128;
export const MAX_RECENT_RESOURCE_ITEMS = 256;
const MAX_RESOURCE_DETAILS_PER_ITEM = 32;
export const MAX_RESOURCE_ENTRIES = 128;

type AgentStatus =
  | "starting"
  | "working"
  | "done"
  | "stopped"
  | "failed"
  /** Its runtime is gone and nothing recorded how it ended. */
  | "unknown";
export type RecentAgent = {
  id: string;
  label: string;
  status: AgentStatus;
  agentKey?: string;
  target?: SubagentPaneTarget;
  /** Settled after its spawn turn; the parent rows hold no end time. */
  endUnknown?: true;
};

function childTarget(
  page: AcpChatPage,
  sessionId: AgentArtifactOwner["sessionId"],
  item: Extract<AcpItem, { kind: "collaboration" }>,
  agentKey: string,
): SubagentPaneTarget | undefined {
  const owner = { projectId: page.projectId, chatId: page.chatId, sessionId };
  return sameAgentArtifactOwner(owner, item) &&
    attributedSubagentKey(item, [agentKey]) === agentKey
    ? { owner, itemId: item.id, agentKey }
    : undefined;
}

export function liveAgent(status: AgentStatus | undefined) {
  return status === "starting" || status === "working";
}

function agentStatus(state: string | undefined): AgentStatus | null {
  if (state === "pendingInit") return "starting";
  if (state === "running") return "working";
  if (state === "completed") return "done";
  if (state === "shutdown" || state === "interrupted") return "stopped";
  if (state === "errored" || state === "notFound") return "failed";
  if (state === "unknown") return state;
  return null;
}

/** The newest turns, holding at most a bounded number of items, oldest first. */
export function recentResourceTurns(page: AcpChatPage): TurnProjection[] {
  let remainingItems = MAX_RECENT_RESOURCE_ITEMS;
  const recentTurns = [];
  for (const turn of page.turns.slice(-MAX_RESOURCE_TURNS).reverse()) {
    if (remainingItems === 0) break;
    const items = turn.items.slice(-remainingItems);
    remainingItems -= items.length;
    recentTurns.push(
      items.length === turn.items.length ? turn : { ...turn, items },
    );
  }
  return recentTurns.reverse();
}

/** The chat's recent subagents, one per spawn invocation, by their latest parent row. */
export function projectRecentAgents(
  page: AcpChatPage | null,
  chatId: string | null,
): RecentAgent[] {
  if (!page || page.chatId !== chatId) return [];
  const agents = new Map<string, RecentAgent>();
  const latestInvocation = new Map<string, string>();
  for (const turn of recentResourceTurns(page)) {
    if (
      page.currentSessionId !== undefined &&
      turn.sessionId !== page.currentSessionId
    )
      continue;
    for (const item of turn.items) {
      if (item.kind === "collaboration") {
        if (item.payload.operation !== "spawnAgent" && item.payload.agentKey) {
          const invocation = latestInvocation.get(item.payload.agentKey);
          const agent = invocation ? agents.get(invocation) : null;
          const target = childTarget(
            page,
            turn.sessionId,
            item,
            item.payload.agentKey,
          );
          const label = item.payload.label?.trim();
          const operation = item.payload.operation;
          const live = liveAgent(agent?.status);
          if (invocation && agent)
            agents.set(invocation, {
              ...agent,
              ...(target
                ? {
                    target,
                    label:
                      agent.label === "Subagent" && label ? label : agent.label,
                  }
                : {}),
              status:
                operation === "interrupted"
                  ? "stopped"
                  : live && operation === "completed"
                    ? "done"
                    : live && operation === "started"
                      ? "working"
                      : agent.status,
            });
        }
        const states = (item.payload.agentStates ?? []).slice(
          0,
          MAX_RESOURCE_DETAILS_PER_ITEM,
        );
        if (item.payload.operation === "spawnAgent") {
          for (const [index, key] of (item.payload.agentKeys ?? [])
            .slice(0, MAX_RESOURCE_DETAILS_PER_ITEM)
            .entries()) {
            const invocation = `${item.id}:${key}`;
            latestInvocation.set(key, invocation);
            const state = states.find(([agent]) => agent === key)?.[1];
            const label =
              item.payload.receiverLabels[index]?.trim() || "Subagent";
            const status = agentStatus(state);
            const target = childTarget(page, turn.sessionId, item, key);
            if (status)
              agents.set(invocation, {
                id: invocation,
                label,
                status,
                agentKey: key,
                ...(target ? { target } : {}),
              });
          }
        } else {
          for (const [key, state] of states) {
            const invocation = latestInvocation.get(key);
            if (!invocation) continue;
            const agent = agents.get(invocation);
            const status = agentStatus(state);
            // A terminal child never returns to working without a new spawn.
            if (
              status &&
              (!agent || liveAgent(agent.status) || !liveAgent(status))
            )
              agents.set(invocation, {
                ...agent,
                id: invocation,
                label:
                  item.payload.receiverLabels[
                    (item.payload.agentKeys ?? []).indexOf(key)
                  ]?.trim() ||
                  agent?.label ||
                  "Subagent",
                status,
                agentKey: key,
              });
          }
        }
      } else if (
        item.kind === "toolCall" &&
        item.payload.presentation.kind === "subagent"
      ) {
        const status = ["pending", "inProgress", "waitingForApproval"].includes(
          item.toolState,
        )
          ? "working"
          : item.toolState === "completed"
            ? "done"
            : null;
        if (status)
          agents.set(item.id, {
            id: item.id,
            label: item.payload.presentation.label,
            status,
          });
      }
    }
  }
  return [...agents.values()].slice(-MAX_RESOURCE_ENTRIES);
}

export function artifactTabParts(tab: ArtifactTab) {
  const full = tab.resource?.relativePath ?? tab.title;
  const base = full.split("/").at(-1) ?? full;
  return [full, base, full.slice(0, full.length - base.length)];
}

export function activeArtifactForTarget<T extends { id: string }>(
  artifacts: readonly T[],
  target: { kind: string; tabId?: string } | null,
): T | null {
  return target?.kind === "tab"
    ? (artifacts.find((artifact) => artifact.id === target.tabId) ?? null)
    : null;
}
