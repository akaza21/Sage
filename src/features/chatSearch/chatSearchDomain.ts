export const MAX_CHAT_SEARCH_QUERY_BYTES = 256;
export const MAX_CHAT_SEARCH_RESULTS = 64;
export const MAX_CHAT_SEARCH_SNIPPET_BYTES = 2 * 1024;
export const MAX_CHAT_SEARCH_RESPONSE_BYTES = 64 * 1024;

const MAX_IDENTIFIER_BYTES = 512;
const MAX_TITLE_BYTES = 1_024;
const MAX_RESPONSE_ELAPSED_MS = 1_000;
const encoder = new TextEncoder();

export interface ProjectChatSearchRequest {
  projectId: string;
  projectGeneration: number;
  expectedTransitionEpoch: number;
  query: string;
}

export interface ProjectChatSearchResult {
  chatId: string;
  title: string;
  attached: boolean;
  matchedTitle: boolean;
  snippet: string | null;
}

export interface ProjectChatSearchResponse {
  projectId: string;
  projectGeneration: number;
  transitionEpoch: number;
  query: string;
  results: ProjectChatSearchResult[];
  hasMore: boolean;
  elapsedMs: number;
}

export interface ProjectChatSearchGateway {
  search(request: ProjectChatSearchRequest): Promise<ProjectChatSearchResponse>;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function hasExactKeys(value: Record<string, unknown>, keys: readonly string[]) {
  const actual = Object.keys(value);
  return actual.length === keys.length && keys.every((key) => key in value);
}

function safeCount(value: unknown): value is number {
  return Number.isSafeInteger(value) && (value as number) >= 0;
}

function containsControl(value: string): boolean {
  return [...value].some((character) => {
    const codePoint = character.codePointAt(0) ?? 0;
    return codePoint <= 0x1f || codePoint === 0x7f;
  });
}

function boundedString(
  value: unknown,
  maximum: number,
  allowControl = false,
): value is string {
  return (
    typeof value === "string" &&
    value.length > 0 &&
    encoder.encode(value).byteLength <= maximum &&
    (allowControl || !containsControl(value))
  );
}

export function validateProjectChatSearchRequest(
  request: ProjectChatSearchRequest,
): void {
  if (!boundedString(request.projectId, MAX_IDENTIFIER_BYTES)) {
    throw new Error("Project Chat search requires a valid Project ID");
  }
  if (
    !safeCount(request.projectGeneration) ||
    !safeCount(request.expectedTransitionEpoch)
  ) {
    throw new Error("Project Chat search requires a valid intent generation");
  }
  const query = request.query.trim();
  if (
    !boundedString(query, MAX_CHAT_SEARCH_QUERY_BYTES) ||
    containsControl(query)
  ) {
    throw new Error("Project Chat search query must be 1-256 UTF-8 bytes");
  }
}

function searchResult(
  value: unknown,
  query: string,
): value is ProjectChatSearchResult {
  if (
    !isRecord(value) ||
    !hasExactKeys(value, [
      "chatId",
      "title",
      "attached",
      "matchedTitle",
      "snippet",
    ]) ||
    !boundedString(value.chatId, MAX_IDENTIFIER_BYTES) ||
    !boundedString(value.title, MAX_TITLE_BYTES) ||
    typeof value.attached !== "boolean" ||
    typeof value.matchedTitle !== "boolean" ||
    (value.snippet !== null &&
      !boundedString(value.snippet, MAX_CHAT_SEARCH_SNIPPET_BYTES, true))
  ) {
    return false;
  }
  const titleMatches = value.title.includes(query);
  const snippetMatches =
    typeof value.snippet === "string" && value.snippet.includes(query);
  return (
    value.matchedTitle === titleMatches &&
    (value.matchedTitle || snippetMatches) &&
    (value.snippet === null || snippetMatches)
  );
}

export function parseProjectChatSearchResponse(
  value: unknown,
): ProjectChatSearchResponse {
  const invalid = () => new Error("invalid Project Chat search response");
  if (
    !isRecord(value) ||
    !hasExactKeys(value, [
      "projectId",
      "projectGeneration",
      "transitionEpoch",
      "query",
      "results",
      "hasMore",
      "elapsedMs",
    ]) ||
    !boundedString(value.projectId, MAX_IDENTIFIER_BYTES) ||
    !safeCount(value.projectGeneration) ||
    !safeCount(value.transitionEpoch) ||
    !boundedString(value.query, MAX_CHAT_SEARCH_QUERY_BYTES) ||
    value.query !== value.query.trim() ||
    !Array.isArray(value.results) ||
    value.results.length > MAX_CHAT_SEARCH_RESULTS ||
    typeof value.hasMore !== "boolean" ||
    !safeCount(value.elapsedMs) ||
    value.elapsedMs > MAX_RESPONSE_ELAPSED_MS
  ) {
    throw invalid();
  }
  const query = value.query;
  if (
    !value.results.every((result): result is ProjectChatSearchResult =>
      searchResult(result, query),
    )
  ) {
    throw invalid();
  }
  const chatIds = new Set(value.results.map((result) => result.chatId));
  if (
    chatIds.size !== value.results.length ||
    encoder.encode(JSON.stringify(value)).byteLength >
      MAX_CHAT_SEARCH_RESPONSE_BYTES
  ) {
    throw invalid();
  }
  return value as unknown as ProjectChatSearchResponse;
}
