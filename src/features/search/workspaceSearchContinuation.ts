import { WORKSPACE_SEARCH_LIMITS } from "./workspaceSearchLimits";

export interface WorkspaceSearchIndexStamp {
  projectId: string;
  transitionEpoch: number;
  projectGeneration: number;
}

interface ContinuationToken {
  v: 1;
  g: number;
  e: number;
  c: 0 | 1;
  w: 0 | 1;
  f: string;
  q: string;
  r: string;
  p: string;
  l: number;
  o: number;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function safeCounter(value: unknown, minimum = 0): value is number {
  return Number.isSafeInteger(value) && (value as number) >= minimum;
}

function boundedIdentifier(value: unknown): value is string {
  return (
    typeof value === "string" &&
    value.length > 0 &&
    !value.includes("\0") &&
    new TextEncoder().encode(value).byteLength <= 512
  );
}

function relativePath(value: unknown): value is string {
  return (
    typeof value === "string" &&
    value.length > 0 &&
    !value.includes("\0") &&
    !value.includes("\\") &&
    !value.startsWith("/") &&
    !value.endsWith("/") &&
    new TextEncoder().encode(value).byteLength <=
      WORKSPACE_SEARCH_LIMITS.resourcePathBytes &&
    value
      .split("/")
      .every((segment) => segment !== "" && segment !== "." && segment !== "..")
  );
}

export function coordinateWorkspaceSearchIndex(
  input: WorkspaceSearchIndexStamp,
): WorkspaceSearchIndexStamp {
  if (!boundedIdentifier(input.projectId)) {
    throw new Error("workspace Search requires a valid Project ID");
  }
  if (!safeCounter(input.transitionEpoch)) {
    throw new Error("workspace Search requires a valid transition epoch");
  }
  if (!safeCounter(input.projectGeneration)) {
    throw new Error("workspace Search requires a valid index generation");
  }
  return {
    projectId: input.projectId,
    transitionEpoch: input.transitionEpoch,
    projectGeneration: input.projectGeneration,
  };
}

function readContinuationToken(value: string): ContinuationToken | null {
  if (
    new TextEncoder().encode(value).byteLength >
    WORKSPACE_SEARCH_LIMITS.continuationTokenBytes
  ) {
    return null;
  }
  let token: unknown;
  try {
    token = JSON.parse(value);
  } catch {
    return null;
  }
  if (!isRecord(token)) return null;
  const expectedKeys: Array<keyof ContinuationToken> = [
    "v",
    "g",
    "e",
    "c",
    "w",
    "f",
    "q",
    "r",
    "p",
    "l",
    "o",
  ];
  if (
    Object.keys(token).length !== expectedKeys.length ||
    !expectedKeys.every((key) => Object.hasOwn(token, key))
  ) {
    return null;
  }
  if (
    token.v !== 1 ||
    !safeCounter(token.g) ||
    !safeCounter(token.e) ||
    (token.c !== 0 && token.c !== 1) ||
    (token.w !== 0 && token.w !== 1) ||
    typeof token.f !== "string" ||
    !/^[a-f0-9]{64}$/.test(token.f) ||
    typeof token.q !== "string" ||
    !/^[a-f0-9]{64}$/.test(token.q) ||
    !boundedIdentifier(token.r) ||
    !relativePath(token.p) ||
    !safeCounter(token.l, 1) ||
    !safeCounter(token.o, 1)
  ) {
    return null;
  }
  return token as unknown as ContinuationToken;
}

export function validWorkspaceSearchContinuationToken(
  value: unknown,
): value is string {
  return typeof value === "string" && readContinuationToken(value) !== null;
}

export function assertWorkspaceSearchContinuationMatchesRequest(
  token: string,
  request: {
    indexGeneration: number;
    expectedTransitionEpoch: number;
    caseSensitive: boolean;
    wholeWord: boolean;
  },
): void {
  const parsed = readContinuationToken(token);
  if (!parsed) {
    throw new Error("workspace Search continuation is invalid");
  }
  if (
    parsed.g !== request.indexGeneration ||
    parsed.e !== request.expectedTransitionEpoch ||
    parsed.c !== (request.caseSensitive ? 1 : 0) ||
    parsed.w !== (request.wholeWord ? 1 : 0)
  ) {
    throw new Error("workspace Search continuation is stale");
  }
}

export function continuationTokenMatchesResponse(
  token: string,
  response: { indexGeneration: number; transitionEpoch: number },
): boolean {
  const parsed = readContinuationToken(token);
  return (
    parsed !== null &&
    parsed.g === response.indexGeneration &&
    parsed.e === response.transitionEpoch
  );
}
