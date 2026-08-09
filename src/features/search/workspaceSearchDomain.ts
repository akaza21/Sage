import type { ResourceRef } from "../../domain/workbench";

export const MAX_WORKSPACE_SEARCH_RESULTS = 500;
export const MAX_WORKSPACE_SEARCH_QUERY_BYTES = 512;
const MAX_PROJECT_ROOTS = 8;
const MAX_IDENTIFIER_BYTES = 512;
const MAX_PATH_BYTES = 32 * 1024;
const MAX_PREVIEW_BYTES = 2 * 1024;
const MAX_MESSAGE_BYTES = 8 * 1024;

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
  projectId: string;
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
  if (!boundedString(value.relativePath, MAX_PATH_BYTES)) return false;
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
    boundedString(value.preview, MAX_PREVIEW_BYTES, true)
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
    !boundedString(value.projectId, MAX_IDENTIFIER_BYTES) ||
    !boundedString(value.query, MAX_WORKSPACE_SEARCH_QUERY_BYTES) ||
    !Array.isArray(value.matches) ||
    value.matches.length > MAX_WORKSPACE_SEARCH_RESULTS ||
    !value.matches.every(searchMatch) ||
    !Array.isArray(value.roots) ||
    value.roots.length > MAX_PROJECT_ROOTS ||
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
  if (
    rootIds.size !== value.roots.length ||
    value.matches.some((match) => !rootIds.has(match.resource.rootId))
  ) {
    throw invalid();
  }
  return value as unknown as WorkspaceSearchResponse;
}
