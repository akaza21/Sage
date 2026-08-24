import { parentResource } from "../../domain/resourceIdentity";
import type { ResourceRef } from "../../domain/workbench";

export const WORKSPACE_NOTIFY_EVENT = "workspace-notify-v1";
export const MAX_IN_FLIGHT_NOTIFY_INVALIDATIONS = 8;
export const MAX_NOTIFY_SEARCH_SUBSCRIBERS = 8;

const NOTICE_KEYS = [
  "projectId",
  "projectGeneration",
  "transitionEpoch",
  "rootId",
  "relativePath",
  "kind",
] as const;

export interface WorkspaceNotifyNotice {
  projectId: string;
  projectGeneration: number;
  transitionEpoch: number;
  rootId: string;
  relativePath: string;
  kind: string;
}

export interface LiveProjectNotifyStamp {
  projectId: string;
  projectGeneration: number;
  transitionEpoch: number;
  roots: Array<{ rootId: string }>;
}

export interface WorkspaceNotifyHooks {
  watch(input: {
    stamp: {
      projectId: string;
      projectGeneration: number;
      transitionEpoch: number;
    };
    roots: Array<{ rootId: string }>;
  }): Promise<{ generation: number }>;
  unwatch(input: { projectId: string }): Promise<void>;
  listen(emit: (value: unknown) => void): Promise<() => void>;
  invalidate(
    notice: WorkspaceNotifyNotice,
    listing: ResourceRef,
  ): void | Promise<void>;
}

interface BoundNotifySession {
  live: LiveProjectNotifyStamp;
  unlisten: () => void;
  inflight: Set<string>;
}

const searchSubscribers = new Set<(notice: WorkspaceNotifyNotice) => void>();
let session: BoundNotifySession | null = null;
let listingAdmission = 0;
let listingAdmissionEpoch = 0;
const listingAdmissionWaiters: Array<() => void> = [];

export async function acquireNotifyListingSlot(): Promise<boolean> {
  const epoch = listingAdmissionEpoch;
  if (listingAdmission < MAX_IN_FLIGHT_NOTIFY_INVALIDATIONS) {
    listingAdmission += 1;
    return epoch === listingAdmissionEpoch;
  }
  await new Promise<void>((resolve) => {
    listingAdmissionWaiters.push(resolve);
  });
  if (epoch !== listingAdmissionEpoch) {
    return false;
  }
  if (listingAdmission >= MAX_IN_FLIGHT_NOTIFY_INVALIDATIONS) {
    return false;
  }
  listingAdmission += 1;
  return true;
}

export function releaseNotifyListingSlot(): void {
  listingAdmission = Math.max(0, listingAdmission - 1);
  const next = listingAdmissionWaiters.shift();
  next?.();
}

export function resetNotifyListingAdmission(): void {
  listingAdmissionEpoch += 1;
  const pending = listingAdmissionWaiters.splice(0);
  for (const resolve of pending) {
    resolve();
  }
}

function safeCounter(value: unknown): number | null {
  return typeof value === "number" &&
    Number.isInteger(value) &&
    value >= 0 &&
    value <= 9_007_199_254_740_991
    ? value
    : null;
}

const MAX_NOTICE_PATH_BYTES = 16 * 1024;
const NOTICE_KINDS = new Set(["created", "changed", "deleted", "resync"]);

function boundedIdentifier(value: unknown): value is string {
  return (
    typeof value === "string" &&
    value.length > 0 &&
    value.length <= 512 &&
    !value.includes("\0")
  );
}

function boundedNotifyPath(value: unknown): value is string {
  if (typeof value !== "string" || value.length === 0 || value.includes("\0")) {
    return false;
  }
  return (
    new TextEncoder().encode(value).length <= MAX_NOTICE_PATH_BYTES &&
    !value.includes("\\") &&
    !value.startsWith("/") &&
    !value.endsWith("/") &&
    value
      .split("/")
      .every((segment) => segment !== "" && segment !== "." && segment !== "..")
  );
}

function boundedNotifyKind(value: unknown): value is string {
  return typeof value === "string" && NOTICE_KINDS.has(value);
}

export function acceptWorkspaceNotifyNotice(
  value: unknown,
  live: LiveProjectNotifyStamp,
): WorkspaceNotifyNotice | null {
  if (!value || typeof value !== "object") {
    return null;
  }
  const record = value as Record<string, unknown>;
  const keys = Object.keys(record);
  if (
    keys.length !== NOTICE_KEYS.length ||
    NOTICE_KEYS.some((key) => !keys.includes(key))
  ) {
    return null;
  }
  const projectGeneration = safeCounter(record.projectGeneration);
  const transitionEpoch = safeCounter(record.transitionEpoch);
  if (
    !boundedIdentifier(record.projectId) ||
    projectGeneration === null ||
    transitionEpoch === null ||
    !boundedIdentifier(record.rootId) ||
    !boundedNotifyPath(record.relativePath) ||
    !boundedNotifyKind(record.kind)
  ) {
    return null;
  }
  if (
    record.projectId !== live.projectId ||
    projectGeneration !== live.projectGeneration ||
    transitionEpoch !== live.transitionEpoch ||
    !live.roots.some((root) => root.rootId === record.rootId)
  ) {
    return null;
  }
  return {
    projectId: record.projectId,
    projectGeneration,
    transitionEpoch,
    rootId: record.rootId,
    relativePath: record.relativePath,
    kind: record.kind,
  };
}

export function listingResourceForNotify(
  notice: WorkspaceNotifyNotice,
): ResourceRef {
  if (notice.kind === "resync") {
    return { rootId: notice.rootId, relativePath: "" };
  }
  return parentResource({
    rootId: notice.rootId,
    relativePath: notice.relativePath,
  });
}

export function subscribeWorkspaceNotifySearch(
  listener: (notice: WorkspaceNotifyNotice) => void,
): () => void {
  if (searchSubscribers.size >= MAX_NOTIFY_SEARCH_SUBSCRIBERS) {
    return () => undefined;
  }
  searchSubscribers.add(listener);
  return () => {
    searchSubscribers.delete(listener);
  };
}

export function publishWorkspaceNotifySearch(
  notice: WorkspaceNotifyNotice,
): void {
  for (const listener of searchSubscribers) {
    listener(notice);
  }
}

export function resetWorkspaceNotifyComposeForTests(): void {
  searchSubscribers.clear();
  resetNotifyListingAdmission();
  listingAdmission = 0;
}

function sameLiveStamp(
  left: LiveProjectNotifyStamp,
  right: LiveProjectNotifyStamp,
): boolean {
  return (
    left.projectId === right.projectId &&
    left.projectGeneration === right.projectGeneration &&
    left.transitionEpoch === right.transitionEpoch &&
    left.roots.length === right.roots.length &&
    left.roots.every(
      (root, index) => root.rootId === right.roots[index]?.rootId,
    )
  );
}

async function dropSession(
  hooks: Pick<WorkspaceNotifyHooks, "unwatch">,
): Promise<void> {
  if (!session) {
    return;
  }
  const projectId = session.live.projectId;
  session.unlisten();
  session = null;
  resetNotifyListingAdmission();
  try {
    await hooks.unwatch({ projectId });
  } catch {
    /* fail-closed: the live Project is already gone */
  }
}

export async function syncLiveProjectWorkspaceNotify(
  live: LiveProjectNotifyStamp | null,
  hooks: WorkspaceNotifyHooks,
): Promise<void> {
  if (!live) {
    await dropSession(hooks);
    return;
  }
  if (session && sameLiveStamp(session.live, live)) {
    return;
  }
  if (session && session.live.projectId !== live.projectId) {
    await dropSession(hooks);
  } else if (session) {
    session.unlisten();
    session = null;
  }
  const inflight = new Set<string>();
  const unlisten = await hooks.listen((value) => {
    if (!session) {
      return;
    }
    const accepted = acceptWorkspaceNotifyNotice(value, session.live);
    if (!accepted) {
      return;
    }
    publishWorkspaceNotifySearch(accepted);
    const listing = listingResourceForNotify(accepted);
    if (accepted.kind === "resync") {
      void Promise.resolve(hooks.invalidate(accepted, listing));
      return;
    }
    const key = `${listing.rootId}\0${listing.relativePath}`;
    if (inflight.has(key)) {
      return;
    }
    if (inflight.size >= MAX_IN_FLIGHT_NOTIFY_INVALIDATIONS) {
      return;
    }
    inflight.add(key);
    void Promise.resolve(hooks.invalidate(accepted, listing)).finally(() => {
      inflight.delete(key);
    });
  });
  session = { live, unlisten, inflight };
  try {
    await hooks.watch({
      stamp: {
        projectId: live.projectId,
        projectGeneration: live.projectGeneration,
        transitionEpoch: live.transitionEpoch,
      },
      roots: live.roots,
    });
  } catch {
    await dropSession(hooks);
  }
}
