import type {
  FileChangeProjection,
  SafeResourceLocation,
  ToolPresentation,
} from "../domain/acp";
import type { ResourceRef } from "../domain/workbench";
import type { FilePairMetadata } from "../domain/agentDiff";
import { utf8ByteLength } from "../domain/text";
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
} from "./acpDesktopGuards";

export const MAX_FILE_CHANGES = MAX_AGENT_DIFFS_PER_TURN;
export const MAX_TOOL_RESOURCES = 64;
export const MAX_SEARCH_RESOURCES = 256;

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
  if (value.displayResource === undefined || value.displayResource === null) {
    return { kind: "unavailable", reason: value.reason, label };
  }
  if (value.reason !== "missing") return null;
  const displayResource = safeResourceRef(value.displayResource);
  if (!displayResource) return null;
  const path = displayResource.relativePath;
  if (
    !path ||
    utf8ByteLength(path) > 512 ||
    path.startsWith("/") ||
    path.includes("\\") ||
    path.includes(":") ||
    path.split("/").some((part) => !part || part === "." || part === "..") ||
    Array.from(path).some((character) => {
      const code = character.charCodeAt(0);
      return code < 32 || (code >= 127 && code <= 159);
    })
  ) {
    return null;
  }
  return { kind: "unavailable", reason: value.reason, label, displayResource };
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

function safeNullableInteger(
  value: unknown,
  minimum: number,
  maximum: number,
): number | null | undefined {
  if (value === undefined || value === null) return null;
  return typeof value === "number" &&
    Number.isSafeInteger(value) &&
    value >= minimum &&
    value <= maximum
    ? value
    : undefined;
}

function safeFlag(value: unknown): boolean | undefined {
  if (value === undefined) return false;
  return typeof value === "boolean" ? value : undefined;
}

function safeToolResources(
  value: unknown,
  limit: number,
): SafeResourceLocation[] | undefined {
  if (!Array.isArray(value) || value.length > limit) return undefined;
  const resources = value.map(safeResourceLocation);
  return resources.some((resource) => !resource)
    ? undefined
    : (resources as SafeResourceLocation[]);
}

function safeOptionalText(
  value: unknown,
  maximumBytes: number,
): string | null | undefined {
  return value === undefined ? null : safeNullableText(value, maximumBytes);
}

function safeOptionalResourceLocation(
  value: unknown,
): SafeResourceLocation | null | undefined {
  if (value === undefined || value === null) return null;
  return safeResourceLocation(value) ?? undefined;
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
    case "webSearch":
      return { kind: "webSearch", label };
    case "read":
    case "edit":
    case "delete":
    case "imageView": {
      const resources = safeToolResources(value.resources, MAX_TOOL_RESOURCES);
      return resources === undefined
        ? null
        : { kind: value.kind, label, resources };
    }
    case "move": {
      const resources = safeToolResources(value.resources, MAX_TOOL_RESOURCES);
      const originalResources = safeToolResources(
        value.originalResources,
        MAX_TOOL_RESOURCES,
      );
      return resources === undefined || originalResources === undefined
        ? null
        : { kind: "move", label, resources, originalResources };
    }
    case "search": {
      const querySummary = safeText(value.querySummary, 16_384);
      const resources = safeToolResources(
        value.resources,
        MAX_SEARCH_RESOURCES,
      );
      const matchCount = safeNullableInteger(
        value.matchCount,
        0,
        4_294_967_295,
      );
      const truncated = safeFlag(value.truncated);
      return querySummary === null ||
        resources === undefined ||
        matchCount === undefined ||
        truncated === undefined
        ? null
        : {
            kind: "search",
            label,
            querySummary,
            resources,
            matchCount,
            truncated,
          };
    }
    case "execute": {
      const summary = safeOptionalText(value.summary, 16_384);
      const cwd = safeOptionalResourceLocation(value.cwd);
      const exitCode = safeNullableInteger(
        value.exitCode,
        -2_147_483_648,
        2_147_483_647,
      );
      const outputTail = safeOptionalText(value.outputTail, 16_384);
      const outputTruncated = safeFlag(value.outputTruncated);
      const resources = safeToolResources(value.resources, MAX_TOOL_RESOURCES);
      const activity =
        value.activity === undefined
          ? "ordinary"
          : isOneOf(value.activity, ["ordinary", "declaredTest"] as const)
            ? value.activity
            : undefined;
      return summary === undefined ||
        cwd === undefined ||
        exitCode === undefined ||
        outputTail === undefined ||
        outputTruncated === undefined ||
        resources === undefined ||
        activity === undefined
        ? null
        : {
            kind: "execute",
            label,
            summary,
            cwd,
            exitCode,
            outputTail,
            outputTruncated,
            resources,
            activity,
          };
    }
    case "fetch": {
      const url = safeOptionalText(value.url, 4_096);
      const summary = safeOptionalText(value.summary, 16_384);
      return url === undefined || summary === undefined
        ? null
        : { kind: "fetch", label, url, summary };
    }
    case "think": {
      const summary = safeOptionalText(value.summary, 16_384);
      return summary === undefined ? null : { kind: "think", label, summary };
    }
    case "switchMode": {
      const modeId = safeOptionalText(value.modeId, 512);
      const modeLabel = safeOptionalText(value.modeLabel, 512);
      return modeId === undefined || modeLabel === undefined
        ? null
        : { kind: "switchMode", label, modeId, modeLabel };
    }
    case "mcp": {
      const serverLabel = safeText(value.serverLabel, 512);
      const toolLabel = safeText(value.toolLabel, 512);
      const argumentsSummary = safeOptionalText(value.argumentsSummary, 16_384);
      const resultSummary = safeOptionalText(value.resultSummary, 16_384);
      return serverLabel === null ||
        toolLabel === null ||
        argumentsSummary === undefined ||
        resultSummary === undefined
        ? null
        : {
            kind: "mcp",
            label,
            serverLabel,
            toolLabel,
            argumentsSummary,
            resultSummary,
          };
    }
    case "subagent": {
      const activityId = safeNullableIdentifier(value.activityId);
      return activityId === undefined ||
        (value.taskLifecycle !== undefined &&
          typeof value.taskLifecycle !== "boolean")
        ? null
        : {
            kind: "subagent",
            label,
            ...(value.taskLifecycle === undefined
              ? {}
              : { taskLifecycle: value.taskLifecycle }),
            activityId: activityId as Extract<
              ToolPresentation,
              { kind: "subagent" }
            >["activityId"],
          };
    }
    case "other": {
      const rawKind = safeOptionalText(value.rawKind, 128);
      const summary = safeOptionalText(value.summary, 16_384);
      return rawKind === undefined || summary === undefined
        ? null
        : { kind: "other", label, rawKind, summary };
    }
    default: {
      const rawKind =
        typeof value.kind === "string"
          ? (safeText(value.kind, 128) ?? null)
          : safeOptionalText(value.rawKind, 128);
      const summary = safeOptionalText(value.summary, 16_384);
      return {
        kind: "other",
        label,
        rawKind: rawKind === undefined ? null : rawKind,
        summary: summary === undefined ? null : summary,
      };
    }
  }
}
