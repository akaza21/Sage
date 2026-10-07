import { resourceKey } from "../../../domain/resourceIdentity";
import {
  parseAgentDiffRef,
  type AgentDiffRef,
} from "../../../domain/agentDiff";
import { MAX_AGENT_DIFFS_PER_TURN } from "../../../domain/agentDiffIdentity";
import type {
  AcpChatPage,
  AcpItem,
  FileChangeProjection,
  TurnProjection,
} from "../../../domain/acp";
import type { ProjectRootSummary } from "../../../domain/workbench";

type SummaryRoot = Pick<ProjectRootSummary, "id" | "name" | "ordinal">;

export function projectRootLabels(
  roots: readonly SummaryRoot[],
): Readonly<Record<string, string>> {
  const counts = new Map<string, number>();
  for (const root of roots)
    counts.set(root.name, (counts.get(root.name) ?? 0) + 1);
  return Object.fromEntries(
    roots.map((root) => [
      root.id,
      (counts.get(root.name) ?? 0) > 1
        ? `${root.name} (root ${root.ordinal + 1})`
        : root.name,
    ]),
  );
}

export function projectRootLabelKey(roots: readonly SummaryRoot[]): string {
  return roots
    .map((root) => `${root.id}\0${root.name}\0${root.ordinal}`)
    .join("\n");
}

export const MAX_AGENT_EDIT_SUMMARY_FILES = MAX_AGENT_DIFFS_PER_TURN;
const MAX_PROJECTED_ITEMS_PER_TURN = 1_000;
const MAX_CHANGES_PER_ITEM = MAX_AGENT_DIFFS_PER_TURN;
const MAX_ID_BYTES = 512;
const MAX_PATH_BYTES = 16 * 1024;
const MAX_SUMMARY_BYTES = 64 * 1024;
const MAX_UNAVAILABLE_LABEL_BYTES = 512;
const MAX_FILE_CHANGE_LINES = 1_000_000;
const MAX_COUNTER_BYTES = 19;
const MAX_COUNTER_VALUE = 9_223_372_036_854_775_807n;

export interface AgentEditSummaryProjection {
  changes: readonly AgentEditSummaryChange[];
  omittedCount: number;
  sourceTruncated: boolean;
  rejectedUnsafeOrStale: boolean;
}

export type AgentEditSummaryChange = FileChangeProjection & {
  diffRef?: AgentDiffRef;
};

function boundedText(value: unknown, maximumBytes: number): value is string {
  return (
    typeof value === "string" &&
    !value.includes("\0") &&
    utf8ByteLength(value) <= maximumBytes
  );
}

function boundedId(value: unknown): value is string {
  return boundedText(value, MAX_ID_BYTES) && value.length > 0;
}

function safeOrdinal(value: unknown): value is number {
  return Number.isSafeInteger(value) && (value as number) >= 0;
}

function safeTimestamp(value: unknown): value is number {
  return Number.isSafeInteger(value);
}

function codeUnitComparison(left: string, right: string): number {
  return left === right ? 0 : left < right ? -1 : 1;
}

function canonicalCounter(value: unknown): value is string {
  return (
    boundedText(value, MAX_COUNTER_BYTES) &&
    /^(?:0|[1-9][0-9]*)$/.test(value) &&
    BigInt(value) <= MAX_COUNTER_VALUE
  );
}

function canonicalRelativePath(value: unknown): value is string {
  return (
    boundedText(value, MAX_PATH_BYTES) &&
    value.length > 0 &&
    !value.startsWith("/") &&
    !value.includes("\\") &&
    value
      .split("/")
      .every((part) => part.length > 0 && part !== "." && part !== "..") &&
    value.charAt(1) !== ":"
  );
}

function safeLocation(location: FileChangeProjection["resource"]): boolean {
  if (location.kind === "resource") {
    return (
      boundedId(location.resource.rootId) &&
      canonicalRelativePath(location.resource.relativePath)
    );
  }
  return (
    [
      "ambiguousRoot",
      "foreign",
      "missing",
      "noncanonical",
      "unsupported",
    ].includes(location.reason) &&
    boundedText(location.label, MAX_UNAVAILABLE_LABEL_BYTES) &&
    (location.displayResource == null ||
      (location.reason === "missing" &&
        boundedId(location.displayResource.rootId) &&
        canonicalRelativePath(location.displayResource.relativePath) &&
        utf8ByteLength(location.displayResource.relativePath) <=
          MAX_UNAVAILABLE_LABEL_BYTES &&
        !Array.from(location.displayResource.relativePath).some((character) => {
          const code = character.charCodeAt(0);
          return code < 32 || code === 127;
        })))
  );
}

function safeLineCount(value: unknown): value is number | null {
  return (
    value === null ||
    (Number.isSafeInteger(value) &&
      (value as number) >= 0 &&
      (value as number) <= MAX_FILE_CHANGE_LINES)
  );
}

function safeChange(change: FileChangeProjection): boolean {
  return (
    safeLocation(change.resource) &&
    (change.originalResource === null ||
      safeLocation(change.originalResource)) &&
    ["created", "updated", "deleted", "renamed", "unknown"].includes(
      change.operation,
    ) &&
    boundedText(change.summary, MAX_SUMMARY_BYTES) &&
    safeLineCount(change.additions) &&
    safeLineCount(change.deletions)
  );
}

function revisionComparison(left: string, right: string): number | null {
  if (!canonicalCounter(left) || !canonicalCounter(right)) return null;
  if (left.length !== right.length) return left.length < right.length ? -1 : 1;
  return left === right ? 0 : left < right ? -1 : 1;
}

function itemOwnershipMatches(
  page: AcpChatPage,
  turn: TurnProjection,
  item: AcpItem,
): boolean {
  if (
    !boundedId(page.projectId) ||
    !boundedId(page.chatId) ||
    !boundedId(turn.id) ||
    !boundedId(turn.sessionId) ||
    !canonicalCounter(turn.runtimeEpoch) ||
    !safeOrdinal(turn.ordinal) ||
    !boundedId(item.id) ||
    !boundedId(item.projectId) ||
    !boundedId(item.chatId) ||
    !boundedId(item.sessionId) ||
    !boundedId(item.turnId) ||
    !canonicalCounter(item.runtimeEpoch) ||
    !safeOrdinal(item.ordinal) ||
    !safeTimestamp(item.updatedAt)
  ) {
    return false;
  }
  const originMatches =
    (item.origin === "primary" && item.activityId === null) ||
    (item.origin === "harnessActivity" &&
      item.activityId !== null &&
      boundedId(item.activityId) &&
      turn.activities.some(
        (activity) =>
          boundedId(activity.id) &&
          boundedId(activity.turnId) &&
          activity.id === item.activityId &&
          activity.turnId === turn.id,
      ));
  return (
    page.sessions.some((session) => session.id === turn.sessionId) &&
    item.projectId === page.projectId &&
    item.chatId === page.chatId &&
    item.sessionId === turn.sessionId &&
    item.turnId === turn.id &&
    item.runtimeEpoch === turn.runtimeEpoch &&
    originMatches
  );
}

function newerItem(current: AcpItem, candidate: AcpItem): AcpItem | null {
  const comparison = revisionComparison(candidate.revision, current.revision);
  if (comparison === null) return null;
  if (comparison !== 0) return comparison > 0 ? candidate : current;
  if (candidate.updatedAt !== current.updatedAt) {
    return candidate.updatedAt > current.updatedAt ? candidate : current;
  }
  if (candidate.ordinal !== current.ordinal) {
    return candidate.ordinal > current.ordinal ? candidate : current;
  }
  return codeUnitComparison(
    JSON.stringify(candidate),
    JSON.stringify(current),
  ) >= 0
    ? candidate
    : current;
}

function changeKey(change: FileChangeProjection): string {
  return change.resource.kind === "resource"
    ? `resource:${resourceKey(change.resource.resource)}`
    : change.resource.displayResource
      ? `unavailable:${change.resource.reason}:${resourceKey(change.resource.displayResource)}`
      : `unavailable:${change.resource.reason}:${change.resource.label}`;
}

export function projectAgentEditSummary(
  page: AcpChatPage,
  turn: TurnProjection,
): AgentEditSummaryProjection {
  let rejectedUnsafeOrStale = false;
  const turnBelongsToPage =
    boundedId(page.projectId) &&
    boundedId(page.chatId) &&
    boundedId(turn.id) &&
    boundedId(turn.sessionId) &&
    canonicalCounter(turn.runtimeEpoch) &&
    safeOrdinal(turn.ordinal) &&
    page.sessions.some(
      (session) => boundedId(session.id) && session.id === turn.sessionId,
    ) &&
    page.turns.some(
      (candidate) =>
        candidate.id === turn.id &&
        candidate.sessionId === turn.sessionId &&
        candidate.runtimeEpoch === turn.runtimeEpoch,
    );
  if (!turnBelongsToPage || turn.items.length > MAX_PROJECTED_ITEMS_PER_TURN) {
    return {
      changes: [],
      omittedCount: 0,
      sourceTruncated: false,
      rejectedUnsafeOrStale: true,
    };
  }

  const itemsById = new Map<string, AcpItem>();
  for (const item of turn.items) {
    if (item.kind !== "fileChangeDiff") continue;
    if (!itemOwnershipMatches(page, turn, item)) {
      rejectedUnsafeOrStale = true;
      continue;
    }
    if (revisionComparison(item.revision, item.revision) === null) {
      rejectedUnsafeOrStale = true;
      continue;
    }
    const current = itemsById.get(item.id);
    if (!current) {
      itemsById.set(item.id, item);
      continue;
    }
    const newest = newerItem(current, item);
    if (newest) itemsById.set(item.id, newest);
    else rejectedUnsafeOrStale = true;
  }

  const ordered = [...itemsById.values()].sort(
    (left, right) =>
      left.ordinal - right.ordinal ||
      left.updatedAt - right.updatedAt ||
      codeUnitComparison(left.id, right.id),
  );
  const changesByResource = new Map<string, AgentEditSummaryChange>();
  let sourceTruncated = false;
  for (const item of ordered) {
    if (item.kind !== "fileChangeDiff") continue;
    sourceTruncated ||= item.payload.truncated;
    if (item.payload.changes.length > MAX_CHANGES_PER_ITEM) {
      rejectedUnsafeOrStale = true;
      continue;
    }
    for (const change of item.payload.changes) {
      if (!safeChange(change)) {
        rejectedUnsafeOrStale = true;
        continue;
      }
      const key = changeKey(change);
      const diffRef =
        change.pairState === "available" &&
        change.resource.kind === "resource" &&
        item.payload.changes.length === 1
          ? parseAgentDiffRef({
              projectId: item.projectId,
              chatId: item.chatId,
              sessionId: item.sessionId,
              runtimeEpoch: item.runtimeEpoch,
              turnId: item.turnId,
              itemId: item.id,
              diffOrdinal: 0,
              snapshotRevision: change.snapshotRevision,
              contentDigest: change.contentDigest,
              ...change.resource.resource,
            })
          : undefined;
      if (change.pairState === "available" && !diffRef) {
        rejectedUnsafeOrStale = true;
        continue;
      }
      changesByResource.delete(key);
      changesByResource.set(key, diffRef ? { ...change, diffRef } : change);
    }
  }

  const allChanges = [...changesByResource.values()];
  const omittedCount = Math.max(
    0,
    allChanges.length - MAX_AGENT_EDIT_SUMMARY_FILES,
  );
  return {
    changes: allChanges.slice(-MAX_AGENT_EDIT_SUMMARY_FILES),
    omittedCount,
    sourceTruncated,
    rejectedUnsafeOrStale,
  };
}
import { utf8ByteLength } from "../../../domain/text";
