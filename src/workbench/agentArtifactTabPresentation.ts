import type { AcpChatPage } from "../domain/acpProjection";
import type { AgentArtifactOwner } from "../domain/agentArtifact";
import type { AgentDiffRef } from "../domain/agentDiff";
import type { ResourceRef } from "../domain/workbench";
import { resourceKey } from "../domain/resourceIdentity";
import { projectAgentEditSummary } from "../features/chat/acp/agentEditSummary";
import {
  attributedSubagentKey,
  type SubagentPaneTarget,
} from "../app/subagentPaneStore";
import { sameAgentArtifactOwner } from "../domain/agentArtifact";
import type { AcpItem } from "../domain/acp";

type ArtifactTab = { resource?: { relativePath?: string }; title: string };

const MAX_RESOURCE_TURNS = 128;
const MAX_RECENT_RESOURCE_ITEMS = 256;
const MAX_RESOURCE_DETAILS_PER_ITEM = 32;
const MAX_RESOURCE_ENTRIES = 128;

export type TaskOutput = {
  label: string;
  owner: AgentArtifactOwner;
  resource: ResourceRef;
  target: "file" | "diff";
  diffRef?: AgentDiffRef;
};

export type TaskSource = { label: string; tabId: string | null };
type AgentStatus = "starting" | "working" | "done" | "stopped" | "failed";
type RecentAgent = {
  id: string;
  label: string;
  status: AgentStatus;
  agentKey?: string;
  target?: SubagentPaneTarget;
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

/** A starting or working child still counts as working. */
export function liveAgent(status: AgentStatus | undefined) {
  return status === "starting" || status === "working";
}

function agentStatus(state: string | undefined): AgentStatus | null {
  if (state === "pendingInit") return "starting";
  if (state === "running") return "working";
  if (state === "completed") return "done";
  if (state === "shutdown" || state === "interrupted") return "stopped";
  if (state === "errored" || state === "notFound") return "failed";
  return null;
}

function httpHost(value: string): string | null {
  if (value.length > 2048) return null;
  try {
    const url = new URL(value);
    return url.protocol === "http:" || url.protocol === "https:"
      ? url.host
      : null;
  } catch {
    return null;
  }
}

export function projectTaskResources(
  page: AcpChatPage | null,
  chatId: string | null,
) {
  const agents = new Map<string, RecentAgent>();
  const latestInvocation = new Map<string, string>();
  const outputs = new Map<string, TaskOutput>();
  const sources = new Map<string, TaskSource>();
  const changes = new Set<string>();
  if (!page || page.chatId !== chatId)
    return {
      recentAgents: [],
      outputs: [],
      sources: [],
      changesCount: 0,
    };
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
  for (const turn of recentTurns.reverse()) {
    for (const change of projectAgentEditSummary(page, turn).changes) {
      const changedResource =
        change.resource.kind === "resource"
          ? change.resource.resource
          : change.resource.displayResource;
      if (
        turn.sessionId === page.currentSessionId &&
        changedResource &&
        changes.size < MAX_RESOURCE_ENTRIES
      )
        changes.add(resourceKey(changedResource));
      if (change.operation !== "created" || change.resource.kind !== "resource")
        continue;
      const resource = change.resource.resource;
      const key = resourceKey(resource);
      if (outputs.size >= MAX_RESOURCE_ENTRIES && !outputs.has(key)) continue;
      outputs.set(key, {
        label: resource.relativePath,
        owner: {
          projectId: page.projectId,
          chatId: page.chatId,
          sessionId: turn.sessionId,
        },
        resource,
        target: change.diffRef ? "diff" : "file",
        diffRef: change.diffRef,
      });
    }
    for (const operation of (turn.browserOperations ?? []).slice(
      -MAX_RECENT_RESOURCE_ITEMS,
    )) {
      if (
        operation.projectId !== page.projectId ||
        operation.chatId !== page.chatId ||
        operation.sessionId !== turn.sessionId ||
        operation.turnId !== turn.id
      )
        continue;
      const label = httpHost(operation.origin);
      if (!label || !operation.tabId) continue;
      if (sources.size >= MAX_RESOURCE_ENTRIES && !sources.has(operation.tabId))
        continue;
      sources.set(operation.tabId, { label, tabId: operation.tabId });
    }
    for (const item of turn.items) {
      if (
        page.currentSessionId !== undefined &&
        turn.sessionId !== page.currentSessionId &&
        (item.kind === "collaboration" ||
          (item.kind === "toolCall" &&
            item.payload.presentation.kind === "subagent"))
      )
        continue;
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
          // Start and completion only advance a live child; an interrupt
          // stops it, and nothing but another spawn revives it.
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
      if (item.kind === "toolCall") {
        // Web pages by host; integrations by the harness's own server name.
        const source = item.payload.presentation;
        const label =
          source.kind === "fetch"
            ? source.url && httpHost(source.url)
            : source.kind === "webSearch"
              ? "Search Service"
              : source.kind === "mcp" && source.serverLabel !== "MCP"
                ? source.serverLabel
                : null;
        if (label && sources.size < MAX_RESOURCE_ENTRIES && !sources.has(label))
          sources.set(label, { label, tabId: null });
      }
    }
  }
  const recentAgents = [...agents.values()].slice(-MAX_RESOURCE_ENTRIES);
  return {
    recentAgents,
    outputs: [...outputs.values()],
    sources: [...sources.values()],
    changesCount: changes.size,
  };
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
