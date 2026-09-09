import type {
  FileChangeProjection,
  SafeResourceLocation,
  ToolPresentation,
} from "../domain/acp";
import type { ResourceRef } from "../domain/workbench";
import type { FilePairMetadata } from "../domain/agentDiff";
import {
  AGENT_DIFF_SIDE_BYTES,
  MAX_AGENT_DIFFS_PER_TURN,
} from "../domain/agentDiffIdentity";
import {
  isOneOf,
  isRecord,
  safeIdentifier,
  safeNullableIdentifier,
  safeNullableText,
  safeText,
  safeCounter,
} from "./acpDesktopResponses";

export const MAX_FILE_CHANGES = MAX_AGENT_DIFFS_PER_TURN;
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
  const pair = safeFilePair(value);
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
    !pair ||
    (pair.pairState === "available" && resource.kind !== "resource") ||
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
    ...pair,
    resource,
    operation: value.operation,
    originalResource,
    summary,
    additions,
    deletions,
  };
}

function safeFilePair(value: Record<string, unknown>): FilePairMetadata | null {
  // Only an entirely absent field set is a legacy summary. A partial new
  // snapshot declaration must never be silently downgraded into an action.
  if (
    value.pairState === undefined &&
    ["snapshotRevision", "oldBytes", "newBytes", "contentDigest"].every(
      (key) => value[key] === undefined,
    )
  )
    return {};
  if (
    value.pairState === "unavailable" ||
    value.pairState === "summaryTruncated"
  ) {
    return value.snapshotRevision === "0" &&
      value.oldBytes === null &&
      value.newBytes === null &&
      value.contentDigest === null
      ? {
          pairState: value.pairState,
          snapshotRevision: "0",
          oldBytes: null,
          newBytes: null,
          contentDigest: null,
        }
      : null;
  }
  const snapshotRevision = safeCounter(value.snapshotRevision, false);
  if (
    value.pairState !== "available" ||
    !snapshotRevision ||
    typeof value.oldBytes !== "number" ||
    !Number.isSafeInteger(value.oldBytes) ||
    value.oldBytes < 0 ||
    value.oldBytes > AGENT_DIFF_SIDE_BYTES ||
    typeof value.newBytes !== "number" ||
    !Number.isSafeInteger(value.newBytes) ||
    value.newBytes < 0 ||
    value.newBytes > AGENT_DIFF_SIDE_BYTES ||
    typeof value.contentDigest !== "string" ||
    !/^[0-9a-f]{64}$/.test(value.contentDigest)
  )
    return null;
  return {
    pairState: "available",
    snapshotRevision,
    oldBytes: value.oldBytes,
    newBytes: value.newBytes,
    contentDigest: value.contentDigest,
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
