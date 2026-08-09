import type {
  ProjectLayoutRequest,
  ResourceRef,
  TabKind,
} from "../domain/workbench";

const MAX_LAYOUT_TABS = 64;
const MAX_RESOURCE_TABS = 32;
const MAX_TAB_METADATA_BYTES = 2 * 1024 * 1024;
const TAB_KINDS = new Set<TabKind>([
  "chat",
  "file",
  "diff",
  "terminal",
  "search",
  "problems",
  "output",
  "settings",
]);

function assertResourceShape(resource: ResourceRef) {
  if (
    typeof resource.rootId !== "string" ||
    typeof resource.relativePath !== "string"
  ) {
    throw new Error("resource identity must use strings");
  }
}

function assertCanonicalResource(
  resource: ResourceRef,
  rootIds: ReadonlySet<string>,
) {
  assertResourceShape(resource);
  if (!rootIds.has(resource.rootId)) {
    throw new Error("layout resource root does not belong to its project");
  }
  const segments = resource.relativePath.split("/");
  if (
    !resource.relativePath ||
    resource.relativePath.includes("\\") ||
    resource.relativePath.startsWith("/") ||
    resource.relativePath.endsWith("/") ||
    /^[A-Za-z]:/u.test(resource.relativePath) ||
    segments.some((segment) => !segment || segment === "." || segment === "..")
  ) {
    throw new Error("path must use one canonical root-relative spelling");
  }
}

function assertTabMetadataBudget(layout: ProjectLayoutRequest) {
  const encoder = new TextEncoder();
  let total = 0;
  for (const tab of layout.tabs) {
    if (tab.resource != null) assertResourceShape(tab.resource);
    const fields = [
      tab.id,
      tab.kind,
      tab.title,
      tab.status ?? "",
      tab.resource?.rootId ?? "",
      tab.resource?.relativePath ?? "",
    ];
    for (const value of fields) {
      if (typeof value !== "string") {
        throw new Error("tab metadata fields must use strings");
      }
      if (value.length > MAX_TAB_METADATA_BYTES) {
        throw new Error("tab projection exceeds its metadata byte limit");
      }
      total += encoder.encode(value).byteLength;
      if (total > MAX_TAB_METADATA_BYTES) {
        throw new Error("tab projection exceeds its metadata byte limit");
      }
    }
  }
}

export function assertBrowserProjectLayout(
  layout: ProjectLayoutRequest,
  projectId: string,
  projectRoots: ReadonlyArray<{ id: string }>,
) {
  if (layout.projectId !== projectId) {
    throw new Error("project layout does not belong to the active Project");
  }
  if (layout.tabs.length > MAX_LAYOUT_TABS) {
    throw new Error(
      `project layout may contain at most ${MAX_LAYOUT_TABS} tabs`,
    );
  }
  if (
    layout.tabs.filter((tab) => tab.kind === "file" || tab.kind === "diff")
      .length > MAX_RESOURCE_TABS
  ) {
    throw new Error(
      `project exceeds its ${MAX_RESOURCE_TABS}-resource-tab restore limit`,
    );
  }
  assertTabMetadataBudget(layout);

  const rootIds = new Set(projectRoots.map((root) => root.id));
  const ids = new Set<string>();
  const positions = new Set<number>();
  for (const tab of layout.tabs) {
    if (!Number.isSafeInteger(tab.position)) {
      throw new Error("tab position must be a safe integer");
    }
    if (typeof tab.pinned !== "boolean" || typeof tab.dirty !== "boolean") {
      throw new Error("tab pinned and dirty fields must be booleans");
    }
    if (ids.has(tab.id) || positions.has(tab.position)) {
      throw new Error(
        "project layout contains duplicate tab identity or position",
      );
    }
    ids.add(tab.id);
    positions.add(tab.position);
    if (!TAB_KINDS.has(tab.kind)) {
      throw new Error("project layout contains an unsupported tab kind");
    }
    if (tab.resource != null) {
      if (tab.kind !== "file" && tab.kind !== "diff") {
        throw new Error("only File and Diff tabs may carry a resource");
      }
      assertCanonicalResource(tab.resource, rootIds);
    } else if (tab.kind === "file" || tab.kind === "diff") {
      throw new Error("File and Diff tabs require a resource identity");
    }
  }

  if (layout.activeTabId !== null && !ids.has(layout.activeTabId)) {
    throw new Error("active tab is absent from the persisted layout");
  }
  if (layout.selectedRootId !== null && !rootIds.has(layout.selectedRootId)) {
    throw new Error("layout resource root does not belong to its project");
  }
  if (layout.selectedResource != null) {
    assertCanonicalResource(layout.selectedResource, rootIds);
    if (layout.selectedRootId !== layout.selectedResource.rootId) {
      throw new Error("selected resource and selected root do not match");
    }
  }
}
