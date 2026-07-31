import type { ResourceRef } from "./workbench";

export function resourceKey(resource: ResourceRef): string {
  return `${resource.rootId.length}:${resource.rootId}${resource.relativePath}`;
}

export function sameResource(
  left: ResourceRef | null | undefined,
  right: ResourceRef | null | undefined,
): boolean {
  return (
    left?.rootId === right?.rootId && left?.relativePath === right?.relativePath
  );
}

export function childResource(rootId: string, relativePath = ""): ResourceRef {
  return { rootId, relativePath };
}

export function parentResource(resource: ResourceRef): ResourceRef {
  const separator = resource.relativePath.lastIndexOf("/");
  return {
    rootId: resource.rootId,
    relativePath:
      separator === -1 ? "" : resource.relativePath.slice(0, separator),
  };
}

export function resourceTitle(resource: ResourceRef): string {
  return resource.relativePath.split("/").at(-1) || resource.relativePath;
}
