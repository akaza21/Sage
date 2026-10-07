import type { AcpChatPage } from "../domain/acpProjection";
import type { AgentArtifactOwner } from "../domain/agentArtifact";
import type { AgentDiffRef } from "../domain/agentDiff";
import type { ResourceRef } from "../domain/workbench";
import { resourceKey } from "../domain/resourceIdentity";
import { projectAgentEditSummary } from "../features/chat/acp/agentEditSummary";

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
  const agents = new Map<
    string,
    { label: string; status: "working" | "done" }
  >();
  const latestInvocation = new Map<string, string>();
  const outputs = new Map<string, TaskOutput>();
  const sources = new Map<string, TaskSource>();
  if (!page || page.chatId !== chatId)
    return {
      workingAgents: 0,
      doneAgents: 0,
      recentAgents: [],
      outputs: [],
      sources: [],
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
    for (const operation of turn.browserOperations.slice(
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
      if (item.kind === "collaboration") {
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
            if (state === "running" || state === "pendingInit")
              agents.set(invocation, { label, status: "working" });
            if (state === "completed")
              agents.set(invocation, { label, status: "done" });
          }
        } else if (
          item.payload.operation === "started" &&
          item.payload.agentKey
        ) {
          const invocation = latestInvocation.get(item.payload.agentKey);
          const agent = invocation ? agents.get(invocation) : null;
          if (invocation && agent?.status !== "done")
            agents.set(invocation, {
              label: agent?.label ?? "Subagent",
              status: "working",
            });
        } else if (
          item.payload.operation === "interrupted" &&
          item.payload.agentKey
        ) {
          const invocation = latestInvocation.get(item.payload.agentKey);
          if (invocation) agents.delete(invocation);
          latestInvocation.delete(item.payload.agentKey);
        }
        if (item.payload.operation !== "spawnAgent") {
          for (const [key, state] of states) {
            const invocation = latestInvocation.get(key);
            if (!invocation) continue;
            const label =
              item.payload.receiverLabels[
                (item.payload.agentKeys ?? []).indexOf(key)
              ]?.trim() ||
              agents.get(invocation)?.label ||
              "Subagent";
            if (
              (state === "running" || state === "pendingInit") &&
              agents.get(invocation)?.status !== "done"
            )
              agents.set(invocation, { label, status: "working" });
            else if (state === "completed")
              agents.set(invocation, { label, status: "done" });
            else if (state !== "running" && state !== "pendingInit") {
              agents.delete(invocation);
              latestInvocation.delete(key);
            }
          }
        }
      } else if (
        item.kind === "toolCall" &&
        item.payload.presentation.kind === "subagent"
      ) {
        const label = item.payload.presentation.label;
        if (
          ["pending", "inProgress", "waitingForApproval"].includes(
            item.toolState,
          )
        )
          agents.set(item.id, { label, status: "working" });
        else if (item.toolState === "completed")
          agents.set(item.id, { label, status: "done" });
      }
      if (
        item.kind === "toolCall" &&
        item.payload.presentation.kind === "fetch"
      ) {
        const label =
          item.payload.presentation.url &&
          httpHost(item.payload.presentation.url);
        if (label && sources.size < MAX_RESOURCE_ENTRIES && !sources.has(label))
          sources.set(label, { label, tabId: null });
      }
    }
  }
  const recentAgents = [...agents.values()].slice(-MAX_RESOURCE_ENTRIES);
  return {
    workingAgents: recentAgents.filter((agent) => agent.status === "working")
      .length,
    doneAgents: recentAgents.filter((agent) => agent.status === "done").length,
    recentAgents,
    outputs: [...outputs.values()],
    sources: [...sources.values()],
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
