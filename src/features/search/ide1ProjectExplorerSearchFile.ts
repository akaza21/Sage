import { resourceKey, sameResource } from "../../domain/resourceIdentity";
import type { ResourceRef } from "../../domain/workbench";
import { WORKSPACE_SEARCH_LIMITS } from "./workspaceSearchLimits";

const MAX_IDENTIFIER_BYTES = 512;
const MAX_POSITION = 0xffff_ffff;
const MAX_COLLECTION = 500;

export interface Ide1JourneyStamp {
  projectId: string;
  projectGeneration: number;
  transitionEpoch: number;
}

export interface Ide1JourneyListing {
  projectId: string;
  projectGeneration: number;
  transitionEpoch: number;
  resources: ResourceRef[];
}

export interface Ide1JourneyMatch {
  resource: ResourceRef;
  lineNumber: number;
  columnNumber: number;
}

export interface Ide1JourneySearch {
  projectId: string;
  indexGeneration: number;
  transitionEpoch: number;
  matches: Ide1JourneyMatch[];
}

export interface Ide1JourneyOpened {
  projectId: string;
  projectGeneration: number;
  transitionEpoch: number;
  resource: ResourceRef;
  lineNumber: number;
  columnNumber: number;
}

export interface Ide1JourneyInput {
  stamp: Ide1JourneyStamp;
  listing: Ide1JourneyListing;
  search: Ide1JourneySearch;
  opened: Ide1JourneyOpened | null;
}

function reject(code: string): never {
  const error = new Error(code);
  (error as Error & { code: string }).code = code;
  throw error;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function exactKeys(
  value: unknown,
  keys: readonly string[],
): value is Record<string, unknown> {
  return (
    isRecord(value) &&
    JSON.stringify(Object.keys(value).sort()) ===
      JSON.stringify([...keys].sort())
  );
}

function boundedIdentifier(value: unknown): value is string {
  return (
    typeof value === "string" &&
    value.length > 0 &&
    !value.includes("\0") &&
    new TextEncoder().encode(value).byteLength <= MAX_IDENTIFIER_BYTES
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

function safeCounter(value: unknown): value is number {
  return Number.isSafeInteger(value) && (value as number) >= 0;
}

function position(value: unknown): value is number {
  return (
    Number.isSafeInteger(value) &&
    (value as number) >= 1 &&
    (value as number) <= MAX_POSITION
  );
}

function resourceRef(value: unknown): value is ResourceRef {
  return (
    exactKeys(value, ["rootId", "relativePath"]) &&
    boundedIdentifier(value.rootId) &&
    relativePath(value.relativePath)
  );
}

function stamp(value: unknown): value is Ide1JourneyStamp {
  return (
    exactKeys(value, ["projectId", "projectGeneration", "transitionEpoch"]) &&
    boundedIdentifier(value.projectId) &&
    safeCounter(value.projectGeneration) &&
    safeCounter(value.transitionEpoch)
  );
}

function listing(value: unknown): value is Ide1JourneyListing {
  if (
    !exactKeys(value, [
      "projectId",
      "projectGeneration",
      "transitionEpoch",
      "resources",
    ]) ||
    !boundedIdentifier(value.projectId) ||
    !safeCounter(value.projectGeneration) ||
    !safeCounter(value.transitionEpoch) ||
    !Array.isArray(value.resources) ||
    value.resources.length < 1 ||
    value.resources.length > MAX_COLLECTION ||
    !value.resources.every(resourceRef)
  ) {
    return false;
  }
  const seen = new Set<string>();
  for (const resource of value.resources) {
    const key = resourceKey(resource);
    if (seen.has(key)) return false;
    seen.add(key);
  }
  return true;
}

function match(value: unknown): value is Ide1JourneyMatch {
  return (
    exactKeys(value, ["resource", "lineNumber", "columnNumber"]) &&
    resourceRef(value.resource) &&
    position(value.lineNumber) &&
    position(value.columnNumber)
  );
}

function search(value: unknown): value is Ide1JourneySearch {
  return (
    exactKeys(value, [
      "projectId",
      "indexGeneration",
      "transitionEpoch",
      "matches",
    ]) &&
    boundedIdentifier(value.projectId) &&
    safeCounter(value.indexGeneration) &&
    safeCounter(value.transitionEpoch) &&
    Array.isArray(value.matches) &&
    value.matches.length >= 1 &&
    value.matches.length <= MAX_COLLECTION &&
    value.matches.every(match)
  );
}

function opened(value: unknown): value is Ide1JourneyOpened {
  return (
    exactKeys(value, [
      "projectId",
      "projectGeneration",
      "transitionEpoch",
      "resource",
      "lineNumber",
      "columnNumber",
    ]) &&
    boundedIdentifier(value.projectId) &&
    safeCounter(value.projectGeneration) &&
    safeCounter(value.transitionEpoch) &&
    resourceRef(value.resource) &&
    position(value.lineNumber) &&
    position(value.columnNumber)
  );
}

function sameMatch(left: Ide1JourneyMatch, right: Ide1JourneyOpened): boolean {
  return (
    sameResource(left.resource, right.resource) &&
    left.lineNumber === right.lineNumber &&
    left.columnNumber === right.columnNumber
  );
}

export function assertIde1ProjectExplorerSearchFileJourney(
  input: Ide1JourneyInput,
): { ok: true } {
  if (
    !exactKeys(input, ["stamp", "listing", "search", "opened"]) ||
    !stamp(input.stamp) ||
    !listing(input.listing) ||
    !search(input.search) ||
    (input.opened !== null && !opened(input.opened))
  ) {
    reject("schema-invalid");
  }
  if (input.opened === null) reject("journey-missing");
  const {
    stamp: journeyStamp,
    listing: journeyListing,
    search: journeySearch,
  } = input;
  if (
    journeyListing.projectId !== journeyStamp.projectId ||
    journeySearch.projectId !== journeyStamp.projectId ||
    input.opened.projectId !== journeyStamp.projectId ||
    journeyListing.projectGeneration !== journeyStamp.projectGeneration ||
    journeySearch.indexGeneration !== journeyStamp.projectGeneration ||
    input.opened.projectGeneration !== journeyStamp.projectGeneration ||
    journeyListing.transitionEpoch !== journeyStamp.transitionEpoch ||
    journeySearch.transitionEpoch !== journeyStamp.transitionEpoch ||
    input.opened.transitionEpoch !== journeyStamp.transitionEpoch
  ) {
    reject("journey-stale");
  }
  const bound = journeySearch.matches.find((item) =>
    sameMatch(item, input.opened),
  );
  if (!bound) reject("journey-substituted");
  if (
    !journeyListing.resources.some((resource) =>
      sameResource(resource, bound.resource),
    )
  ) {
    reject("journey-substituted");
  }
  return { ok: true };
}
