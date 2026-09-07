import type {
  FileChangeProjection,
  SafeResourceLocation,
  ToolPresentation,
} from "../domain/acp";
import type { ResourceRef } from "../domain/workbench";
import {
  isOneOf,
  isRecord,
  safeIdentifier,
  safeNullableIdentifier,
  safeNullableText,
  safeText,
} from "./acpDesktopResponses";

export const MAX_FILE_CHANGES = 64;
export const MAX_TOOL_RESOURCES = 64;

export function safeResourceRef(value: unknown): ResourceRef | null {
  if (!isRecord(value)) return null;
  const rootId = safeIdentifier(value.rootId);
  const relativePath = safeText(value.relativePath, 16_384);
  return rootId && relativePath !== null ? { rootId, relativePath } : null;
}

export function safeResourceLocation(
  value: unknown,
): SafeResourceLocation | null {
  if (!isRecord(value)) return null;
  if (value.kind === "resource") {
    const resource = safeResourceRef(value.resource);
    return resource ? { kind: "resource", resource } : null;
  }
  const label = safeText(value.label, 512);
  if (
    label === null ||
    !isOneOf(value.kind, ["unavailable"] as const) ||
    !isOneOf(value.reason, [
      "ambiguousRoot",
      "foreign",
      "missing",
      "noncanonical",
      "unsupported",
    ] as const)
  ) {
    return null;
  }
  return { kind: "unavailable", reason: value.reason, label };
}

function safeNullableBoundedNumber(value: unknown): number | null | undefined {
  if (value === null) return null;
  return typeof value === "number" &&
    Number.isSafeInteger(value) &&
    value >= 0 &&
    value <= 1_000_000
    ? value
    : undefined;
}

export function safeFileChange(value: unknown): FileChangeProjection | null {
  if (!isRecord(value)) return null;
  const resource = safeResourceLocation(value.resource);
  const originalResource =
    value.originalResource === null
      ? null
      : (safeResourceLocation(value.originalResource) ?? undefined);
  const summary = safeText(value.summary, 65_536);
  const additions = safeNullableBoundedNumber(value.additions);
  const deletions = safeNullableBoundedNumber(value.deletions);
  if (
    !resource ||
    originalResource === undefined ||
    summary === null ||
    additions === undefined ||
    deletions === undefined ||
    !isOneOf(value.operation, [
      "created",
      "updated",
      "deleted",
      "renamed",
      "unknown",
    ] as const)
  ) {
    return null;
  }
  return {
    resource,
    operation: value.operation,
    originalResource,
    summary,
    additions,
    deletions,
  };
}

export function safeToolPresentation(value: unknown): ToolPresentation | null {
  if (!isRecord(value)) return null;
  const label = safeText(value.label, 512);
  if (label === null) return null;
  switch (value.kind) {
    case "generic":
    case "web": {
      const summary = safeNullableText(value.summary, 16_384);
      return summary === undefined
        ? null
        : { kind: value.kind, label, summary };
    }
    case "command": {
      const summary = safeNullableText(value.summary, 16_384);
      if (
        summary === undefined ||
        !Array.isArray(value.resources) ||
        value.resources.length > MAX_TOOL_RESOURCES
      ) {
        return null;
      }
      const resources = value.resources.map(safeResourceLocation);
      return resources.some((resource) => !resource)
        ? null
        : {
            kind: "command",
            label,
            summary,
            resources: resources as SafeResourceLocation[],
          };
    }
    case "file":
    case "diff": {
      if (
        !Array.isArray(value.resources) ||
        value.resources.length > MAX_TOOL_RESOURCES
      ) {
        return null;
      }
      const resources = value.resources.map(safeResourceLocation);
      return resources.some((resource) => !resource)
        ? null
        : {
            kind: value.kind,
            label,
            resources: resources as SafeResourceLocation[],
          };
    }
    case "search": {
      const querySummary = safeText(value.querySummary, 16_384);
      if (
        querySummary === null ||
        !Array.isArray(value.resources) ||
        value.resources.length > MAX_TOOL_RESOURCES
      ) {
        return null;
      }
      const resources = value.resources.map(safeResourceRef);
      return resources.some((resource) => !resource)
        ? null
        : {
            kind: "search",
            label,
            querySummary,
            resources: resources as ResourceRef[],
          };
    }
    case "mcp": {
      const serverLabel = safeText(value.serverLabel, 512);
      const toolLabel = safeText(value.toolLabel, 512);
      return serverLabel === null || toolLabel === null
        ? null
        : { kind: "mcp", label, serverLabel, toolLabel };
    }
    case "subagent": {
      const activityId = safeNullableIdentifier(value.activityId);
      return activityId === undefined
        ? null
        : {
            kind: "subagent",
            label,
            activityId: activityId as Extract<
              ToolPresentation,
              { kind: "subagent" }
            >["activityId"],
          };
    }
    default:
      return null;
  }
}
