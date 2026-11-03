import type { AcpChatPage } from "../domain/acp";
import type {
  AgentArtifactAdmission,
  AgentArtifactOpenIntent,
  AgentArtifactOwner,
} from "../domain/agentArtifact";
import { sameAgentArtifactOwner } from "../domain/agentArtifact";
import type { ResourceRef, WorkbenchTab } from "../domain/workbench";
import { resourceKey } from "../domain/resourceIdentity";
import { projectAgentEditSummary } from "../features/chat/acp/agentEditSummary";

export type AgentToolResource = {
  resource: ResourceRef;
  label: string;
  intent?: AgentArtifactOpenIntent;
  tabId?: string;
};
export function agentToolResources(
  tool: "changes" | "files",
  page: AcpChatPage | null,
  owner: AgentArtifactOwner | null,
  tabs: readonly WorkbenchTab[],
  admissions: readonly AgentArtifactAdmission[],
): AgentToolResource[] {
  if (
    !page ||
    !owner ||
    page.projectId !== owner.projectId ||
    page.chatId !== owner.chatId ||
    page.currentSessionId !== owner.sessionId
  )
    return [];
  const entries = new Map<string, AgentToolResource>();
  for (const turn of page.turns.slice(-32).reverse()) {
    if (turn.sessionId !== owner.sessionId) continue;
    for (const change of projectAgentEditSummary(page, turn).changes) {
      if (change.resource.kind !== "resource") continue;
      const resource = change.resource.resource;
      const key = resourceKey(resource);
      if (entries.has(key)) continue;
      const available =
        tool === "changes"
          ? Boolean(change.diffRef)
          : change.operation !== "deleted";
      entries.set(key, {
        resource,
        label: resource.relativePath,
        ...(available
          ? {
              intent: {
                owner,
                target: tool === "changes" ? "diff" : "file",
                resource,
                ...(tool === "changes" && change.diffRef
                  ? { diffRef: change.diffRef }
                  : {}),
              } as AgentArtifactOpenIntent,
            }
          : {}),
      });
      if (entries.size === 128) return [...entries.values()];
    }
  }
  for (const admission of admissions.slice(-32)) {
    if (!sameAgentArtifactOwner(admission.owner, owner)) continue;
    const tab = tabs.find(
      (candidate) =>
        candidate.id === admission.tabId &&
        candidate.kind === (tool === "changes" ? "diff" : "file"),
    );
    if (!tab?.resource || entries.has(resourceKey(tab.resource))) continue;
    entries.set(resourceKey(tab.resource), {
      resource: tab.resource,
      label: tab.resource.relativePath,
      tabId: tab.id,
    });
    if (entries.size === 128) break;
  }
  return [...entries.values()];
}
