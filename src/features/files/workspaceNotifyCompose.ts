import { isShortId, safeOrdinal } from "../../domain/guards";
import { parentResource } from "../../domain/resourceIdentity";
import type { ResourceRef } from "../../domain/workbench";

export const WORKSPACE_NOTIFY_EVENT = "workspace-notify-v2";
/** The native watcher sends at most this many notices in one event. */
export const MAX_NOTIFY_BATCH_NOTICES = 256;
export const MAX_IN_FLIGHT_NOTIFY_INVALIDATIONS = 8;
/** Folder refreshes waiting for an in-flight slot; past this, the root resyncs. */
export const MAX_QUEUED_NOTIFY_INVALIDATIONS = 64;
export const MAX_NOTIFY_LISTING_WAITERS = 64;
export const MAX_NOTIFY_SEARCH_SUBSCRIBERS = 8;

/** A `.gitignore` or `.ignore` file: a change re-dims every listing below. */
export function isIgnoreRuleFile(relativePath: string): boolean {
  return /(?:^|\/)\.(?:git)?ignore$/i.test(relativePath);
}

/**
 * The notices of one native event, `{ notices: [...] }`. Each is still checked
 * against the live Project by `acceptWorkspaceNotifyNotice`; an event that is
 * not a bounded batch yields nothing.
 */
export function workspaceNotifyBatchNotices(value: unknown): unknown[] {
  if (!value || typeof value !== "object") {
    return [];
  }
  const record = value as Record<string, unknown>;
  const keys = Object.keys(record);
  const notices = record.notices;
  if (
    keys.length !== 1 ||
    !Array.isArray(notices) ||
    notices.length > MAX_NOTIFY_BATCH_NOTICES
  ) {
    return [];
  }
  return notices;
}

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
}

interface ListingRefresh {
  notice: WorkspaceNotifyNotice;
  listing: ResourceRef;
}

/**
 * Runs folder refreshes for one bound session: at most
 * MAX_IN_FLIGHT_NOTIFY_INVALIDATIONS at once and one per folder. A notice for
 * a folder that is refreshing marks it dirty, so it refreshes once more when
 * the running refresh ends, since that one may have listed the folder before
 * the change. A folder with no free slot waits in a bounded queue; past that
 * bound the root refreshes as a whole, once.
 */
function createListingRefreshes(
  invalidate: WorkspaceNotifyHooks["invalidate"],
  live: () => boolean,
) {
  const running = new Map<string, { dirty: ListingRefresh | null }>();
  const queued = new Map<string, ListingRefresh>();

  const start = (key: string, refresh: ListingRefresh) => {
    const entry = { dirty: null as ListingRefresh | null };
    running.set(key, entry);
    let pending: Promise<void>;
    try {
      pending = Promise.resolve(invalidate(refresh.notice, refresh.listing));
    } catch {
      pending = Promise.resolve();
    }
    void pending
      .catch(() => undefined)
      .finally(() => {
        running.delete(key);
        if (!live()) {
          return;
        }
        if (entry.dirty) {
          start(key, entry.dirty);
          return;
        }
        const next = queued.entries().next();
        if (!next.done) {
          const [nextKey, nextRefresh] = next.value;
          queued.delete(nextKey);
          start(nextKey, nextRefresh);
        }
      });
  };

  return (refresh: ListingRefresh) => {
    const { listing, notice } = refresh;
    const key = `${listing.rootId}\0${listing.relativePath}`;
    const current = running.get(key);
    if (current) {
      current.dirty = refresh;
      return;
    }
    if (running.size < MAX_IN_FLIGHT_NOTIFY_INVALIDATIONS) {
      start(key, refresh);
      return;
    }
    if (queued.has(key) || queued.size < MAX_QUEUED_NOTIFY_INVALIDATIONS) {
      queued.set(key, refresh);
      return;
    }
    void Promise.resolve(
      invalidate(
        { ...notice, relativePath: LOST_NOTICES_PATH, kind: "resync" },
        { rootId: listing.rootId, relativePath: "" },
      ),
    );
  };
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
  if (listingAdmissionWaiters.length >= MAX_NOTIFY_LISTING_WAITERS) {
    return false;
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

const MAX_NOTICE_PATH_BYTES = 16 * 1024;
/** The path the native watcher gives a resync notice. */
const LOST_NOTICES_PATH = "lost-notices";
const NOTICE_KINDS = new Set(["created", "changed", "deleted", "resync"]);

function boundedNotifyPath(value: unknown): value is string {
  if (typeof value !== "string" || value.length === 0 || value.includes("\0")) {
    return false;
  }
  return (
    utf8ByteLength(value) <= MAX_NOTICE_PATH_BYTES &&
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
  const projectGeneration = safeOrdinal(record.projectGeneration);
  const transitionEpoch = safeOrdinal(record.transitionEpoch);
  if (
    !isShortId(record.projectId) ||
    projectGeneration === null ||
    transitionEpoch === null ||
    !isShortId(record.rootId) ||
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
  let bound: BoundNotifySession | null = null;
  const refreshListing = createListingRefreshes(
    hooks.invalidate,
    () => bound !== null && session === bound,
  );
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
    // A rule-file change refreshes the whole root, so it must not be
    // coalesced into a refresh of its folder that is already running.
    if (accepted.kind === "resync" || isIgnoreRuleFile(accepted.relativePath)) {
      void Promise.resolve(hooks.invalidate(accepted, listing));
      return;
    }
    refreshListing({ notice: accepted, listing });
  });
  bound = { live, unlisten };
  session = bound;
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
import { utf8ByteLength } from "../../domain/text";
