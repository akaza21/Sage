import { isNestedRepository, sameRepository } from "../domain/diffIdentity";
import {
  resourceKey,
  resourceTitle,
  sameResource,
} from "../domain/resourceIdentity";
import type { RepositoryRef, ResourceRef } from "../domain/workbench";
import type { WorkbenchState } from "./store";
import type { AgentDiffRef } from "../domain/agentDiff";
import { agentDiffKey, isSnapshotDiffTab } from "../domain/agentDiffIdentity";

const MAX_OPEN_RESOURCE_TABS = 32;

export function resourceTabUpdate(
  state: WorkbenchState,
  kind: "diff" | "file",
  resource: ResourceRef,
  repository?: RepositoryRef,
  diffRef?: AgentDiffRef,
): Partial<WorkbenchState> {
  if (
    diffRef &&
    (kind !== "diff" ||
      repository ||
      diffRef.rootId !== resource.rootId ||
      diffRef.relativePath !== resource.relativePath)
  ) {
    return {
      error:
        "Snapshot Diff tabs require their exact resource and no Git repository.",
    };
  }
  const existing = state.tabs.find(
    (tab) =>
      tab.kind === kind &&
      sameResource(tab.resource, resource) &&
      (diffRef
        ? Boolean(
            tab.diffRef && agentDiffKey(tab.diffRef) === agentDiffKey(diffRef),
          )
        : !isSnapshotDiffTab(tab)) &&
      sameRepository(tab.repository, repository),
  );
  if (existing) {
    return {
      activeTabId: existing.id,
      selectedResource: resource,
      selectedRootId: resource.rootId,
      sidebarOpen: true,
      sidebarView: "explorer",
    };
  }

  const resourceTabs = state.tabs.filter(
    (tab) => tab.kind === "file" || tab.kind === "diff",
  );
  const evicted =
    resourceTabs.length >= MAX_OPEN_RESOURCE_TABS
      ? resourceTabs
          .filter(
            (tab) => !tab.pinned && !tab.dirty && tab.id !== state.activeTabId,
          )
          .sort((left, right) => left.position - right.position)[0]
      : undefined;
  if (resourceTabs.length >= MAX_OPEN_RESOURCE_TABS && !evicted) {
    return {
      error:
        "The 32 resource-tab limit is reached. Save or discard a dirty tab before opening another resource.",
    };
  }
  const retainedTabs = evicted
    ? state.tabs.filter((tab) => tab.id !== evicted.id)
    : state.tabs;
  const position =
    retainedTabs.reduce((maximum, tab) => Math.max(maximum, tab.position), -1) +
    1;
  const id = diffRef
    ? `diff:snapshot:${crypto.randomUUID()}`
    : isNestedRepository(repository)
      ? `${kind}:${resourceKey(repository)}:${resourceKey(resource)}`
      : `${kind}:${resourceKey(resource)}`;

  return {
    ...(evicted?.diffRef
      ? {
          diffs: Object.fromEntries(
            Object.entries(state.diffs).filter(
              ([key]) => key !== agentDiffKey(evicted.diffRef!),
            ),
          ),
        }
      : {}),
    tabs: [
      ...retainedTabs,
      {
        id,
        kind,
        title: resourceTitle(resource),
        position,
        pinned: false,
        dirty: false,
        resource,
        ...(diffRef ? { diffRef } : {}),
        ...(isNestedRepository(repository) ? { repository } : {}),
      },
    ],
    activeTabId: id,
    selectedResource: resource,
    selectedRootId: resource.rootId,
    sidebarOpen: true,
    sidebarView: "explorer",
  };
}
