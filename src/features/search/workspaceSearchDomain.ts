import type { ResourceRef } from "../../domain/workbench";
import { WORKSPACE_SEARCH_LIMITS } from "./workspaceSearchLimits";

export const MAX_WORKSPACE_SEARCH_RESULTS = WORKSPACE_SEARCH_LIMITS.results;
export const MAX_WORKSPACE_SEARCH_QUERY_BYTES =
  WORKSPACE_SEARCH_LIMITS.queryBytes;
const MAX_IDENTIFIER_BYTES = 512;
const MAX_MESSAGE_BYTES = 8 * 1024;
const MAX_REQUEST_ID_BYTES = 128;

export type WorkspaceSearchLimitReason =
  | "resultLimit"
  | "fileLimit"
  | "fileSizeLimit"
  | "byteLimit"
  | "entryLimit"
  | "depthLimit"
  | "timeLimit";

export interface WorkspaceSearchRequest {
  projectId: string;
  expectedTransitionEpoch: number;
  query: string;
  caseSensitive: boolean;
}

export interface WorkspaceSearchMatch {
  resource: ResourceRef;
  lineNumber: number;
  columnNumber: number;
  preview: string;
}

export interface WorkspaceSearchRootReport {
  rootId: string;
  rootName: string;
  state: "complete" | "partial" | "unavailable";
  filesScanned: number;
  bytesScanned: number;
  entriesVisited: number;
  skippedFiles: number;
  errorCount: number;
  message: string | null;
}

export interface WorkspaceSearchResponse {
  requestId: string;
  projectId: string;
  transitionEpoch: number;
  query: string;
  matches: WorkspaceSearchMatch[];
  roots: WorkspaceSearchRootReport[];
  truncated: boolean;
  limitReasons: WorkspaceSearchLimitReason[];
  elapsedMs: number;
}

export interface WorkspaceSearchGateway {
  search(
    request: WorkspaceSearchRequest,
    signal?: AbortSignal,
  ): Promise<WorkspaceSearchResponse>;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function boundedString(
  value: unknown,
  maxBytes: number,
  allowEmpty = false,
): value is string {
  return (
    typeof value === "string" &&
    (allowEmpty || value.length > 0) &&
    !value.includes("\0") &&
    new TextEncoder().encode(value).byteLength <= maxBytes
  );
}

function safeCount(value: unknown, minimum = 0): value is number {
  return Number.isSafeInteger(value) && (value as number) >= minimum;
}

function canonicalResource(value: unknown): value is ResourceRef {
  if (!isRecord(value)) return false;
  if (!boundedString(value.rootId, MAX_IDENTIFIER_BYTES)) return false;
  if (
    !boundedString(
      value.relativePath,
      WORKSPACE_SEARCH_LIMITS.resourcePathBytes,
    )
  )
    return false;
  const path = value.relativePath;
  return (
    !path.includes("\\") &&
    !path.startsWith("/") &&
    !path.endsWith("/") &&
    path
      .split("/")
      .every((segment) => segment !== "" && segment !== "." && segment !== "..")
  );
}

function searchMatch(value: unknown): value is WorkspaceSearchMatch {
  return (
    isRecord(value) &&
    canonicalResource(value.resource) &&
    safeCount(value.lineNumber, 1) &&
    safeCount(value.columnNumber, 1) &&
    boundedString(value.preview, WORKSPACE_SEARCH_LIMITS.previewBytes, true)
  );
}

function rootReport(value: unknown): value is WorkspaceSearchRootReport {
  return (
    isRecord(value) &&
    boundedString(value.rootId, MAX_IDENTIFIER_BYTES) &&
    boundedString(value.rootName, MAX_IDENTIFIER_BYTES) &&
    (value.state === "complete" ||
      value.state === "partial" ||
      value.state === "unavailable") &&
    safeCount(value.filesScanned) &&
    safeCount(value.bytesScanned) &&
    safeCount(value.entriesVisited) &&
    safeCount(value.skippedFiles) &&
    safeCount(value.errorCount) &&
    (value.message === null || boundedString(value.message, MAX_MESSAGE_BYTES))
  );
}

const limitReasons: readonly WorkspaceSearchLimitReason[] = [
  "resultLimit",
  "fileLimit",
  "fileSizeLimit",
  "byteLimit",
  "entryLimit",
  "depthLimit",
  "timeLimit",
];

function limitReason(value: unknown): value is WorkspaceSearchLimitReason {
  return limitReasons.includes(value as WorkspaceSearchLimitReason);
}

export function validateWorkspaceSearchRequest(
  request: WorkspaceSearchRequest,
): void {
  if (!boundedString(request.projectId, MAX_IDENTIFIER_BYTES)) {
    throw new Error("workspace Search requires a valid Project ID");
  }
  if (!safeCount(request.expectedTransitionEpoch)) {
    throw new Error("workspace Search requires a valid transition epoch");
  }
  const query = request.query.trim();
  if (!boundedString(query, MAX_WORKSPACE_SEARCH_QUERY_BYTES)) {
    throw new Error(
      `workspace Search query must be 1-${MAX_WORKSPACE_SEARCH_QUERY_BYTES} bytes`,
    );
  }
}

export function parseWorkspaceSearchResponse(
  value: unknown,
): WorkspaceSearchResponse {
  const invalid = () => new Error("invalid workspace Search response");
  if (
    !isRecord(value) ||
    !boundedString(value.requestId, MAX_REQUEST_ID_BYTES) ||
    !/^[A-Za-z0-9_:-]+$/.test(value.requestId) ||
    !boundedString(value.projectId, MAX_IDENTIFIER_BYTES) ||
    !safeCount(value.transitionEpoch) ||
    !boundedString(value.query, MAX_WORKSPACE_SEARCH_QUERY_BYTES) ||
    !Array.isArray(value.matches) ||
    value.matches.length > MAX_WORKSPACE_SEARCH_RESULTS ||
    !value.matches.every(searchMatch) ||
    !Array.isArray(value.roots) ||
    value.roots.length > WORKSPACE_SEARCH_LIMITS.projectRoots ||
    !value.roots.every(rootReport) ||
    typeof value.truncated !== "boolean" ||
    !Array.isArray(value.limitReasons) ||
    value.limitReasons.length > limitReasons.length ||
    !value.limitReasons.every(limitReason) ||
    new Set(value.limitReasons).size !== value.limitReasons.length ||
    !safeCount(value.elapsedMs)
  ) {
    throw invalid();
  }
  const rootIds = new Set(value.roots.map((root) => root.rootId));
  const responseLimitReasons = value.limitReasons;
  const unavailableRootIds = new Set(
    value.roots
      .filter((root) => root.state === "unavailable")
      .map((root) => root.rootId),
  );
  const hasPartialRoot = value.roots.some((root) => root.state === "partial");
  const expectedTruncated = responseLimitReasons.length > 0 || hasPartialRoot;
  const totalFiles = value.roots.reduce(
    (total, root) => total + root.filesScanned,
    0,
  );
  const totalBytes = value.roots.reduce(
    (total, root) => total + root.bytesScanned,
    0,
  );
  const totalEntries = value.roots.reduce(
    (total, root) => total + root.entriesVisited,
    0,
  );
  const contradictoryRoot = value.roots.some((root) => {
    if (
      root.filesScanned > WORKSPACE_SEARCH_LIMITS.files ||
      root.bytesScanned > WORKSPACE_SEARCH_LIMITS.totalBytes ||
      root.entriesVisited > WORKSPACE_SEARCH_LIMITS.entries
    ) {
      return true;
    }
    if (root.state === "complete") {
      return root.errorCount !== 0 || root.message !== null;
    }
    if (root.state === "unavailable") {
      return (
        root.filesScanned !== 0 ||
        root.bytesScanned !== 0 ||
        root.entriesVisited !== 0 ||
        root.skippedFiles !== 0 ||
        root.errorCount < 1 ||
        root.message === null
      );
    }
    return (
      root.errorCount > 0 !== (root.message !== null) ||
      (root.errorCount === 0 &&
        root.skippedFiles === 0 &&
        responseLimitReasons.length === 0)
    );
  });
  if (
    rootIds.size !== value.roots.length ||
    totalFiles > WORKSPACE_SEARCH_LIMITS.files ||
    totalBytes > WORKSPACE_SEARCH_LIMITS.totalBytes ||
    totalEntries > WORKSPACE_SEARCH_LIMITS.entries ||
    value.matches.some(
      (match) =>
        !rootIds.has(match.resource.rootId) ||
        unavailableRootIds.has(match.resource.rootId),
    ) ||
    value.truncated !== expectedTruncated ||
    contradictoryRoot ||
    value.elapsedMs > WORKSPACE_SEARCH_LIMITS.responseElapsedMs ||
    (responseLimitReasons.includes("resultLimit") &&
      value.matches.length !== WORKSPACE_SEARCH_LIMITS.results)
  ) {
    throw invalid();
  }
  return value as unknown as WorkspaceSearchResponse;
}
