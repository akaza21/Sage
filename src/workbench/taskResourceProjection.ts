import type { AcpChatPage } from "../domain/acpProjection";
import type { AgentArtifactOwner } from "../domain/agentArtifact";
import type { AgentDiffRef } from "../domain/agentDiff";
import type { ResourceRef } from "../domain/workbench";
import { resourceKey } from "../domain/resourceIdentity";
import {
  projectAgentEditSummary,
  projectChangeResource,
} from "../features/chat/acp/agentEditSummary";
import {
  MAX_RECENT_RESOURCE_ITEMS,
  MAX_RESOURCE_ENTRIES,
  recentResourceTurns,
} from "./agentArtifactTabPresentation";

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

/** The chat's recent changes, created outputs and sources, for its resources panel. */
export function projectTaskResources(
  page: AcpChatPage | null,
  chatId: string | null,
) {
  const outputs = new Map<string, TaskOutput>();
  const sources = new Map<string, TaskSource>();
  const changes = new Set<string>();
  if (!page || page.chatId !== chatId)
    return { outputs: [], sources: [], changesCount: 0 };
  for (const turn of recentResourceTurns(page)) {
    for (const change of projectAgentEditSummary(page, turn).changes) {
      const changedResource = projectChangeResource(change);
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
      if (item.kind !== "toolCall") continue;
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
  return {
    outputs: [...outputs.values()],
    sources: [...sources.values()],
    changesCount: changes.size,
  };
}
