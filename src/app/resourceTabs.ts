import { isNestedRepository, sameRepository } from "../domain/diffIdentity";
import {
  resourceKey,
  resourceTitle,
  sameResource,
} from "../domain/resourceIdentity";
import type { RepositoryRef, ResourceRef } from "../domain/workbench";
import type { WorkbenchState } from "./store";

const MAX_OPEN_RESOURCE_TABS = 32;

export function resourceTabUpdate(
  state: WorkbenchState,
  kind: "diff" | "file",
  resource: ResourceRef,
  repository?: RepositoryRef,
): Partial<WorkbenchState> {
  const id = isNestedRepository(repository)
    ? `${kind}:${resourceKey(repository)}:${resourceKey(resource)}`
    : `${kind}:${resourceKey(resource)}`;
  const existing = state.tabs.find(
    (tab) =>
      tab.kind === kind &&
      sameResource(tab.resource, resource) &&
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

  return {
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
