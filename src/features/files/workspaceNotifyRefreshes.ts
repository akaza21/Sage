import type { ResourceRef } from "../../domain/workbench";
import type {
  WorkspaceNotifyHooks,
  WorkspaceNotifyNotice,
} from "./workspaceNotifyCompose";

export const MAX_QUEUED_NOTIFY_INVALIDATIONS = 64;
/**
 * A `cargo build` with `target` expanded changes it many times a second; once a
 * second keeps the listing current.
 */
export const IGNORED_SUBTREE_REFRESH_MS = 1_000;
export const MAX_THROTTLED_IGNORED_FOLDERS = 64;

/** The path the native watcher gives a resync notice. */
export const LOST_NOTICES_PATH = "lost-notices";

export interface ListingRefresh {
  notice: WorkspaceNotifyNotice;
  listing: ResourceRef;
}

const listingKey = (listing: ResourceRef) =>
  `${listing.rootId}\0${listing.relativePath}`;

function settled(run: () => void | Promise<void>): Promise<void> {
  try {
    return Promise.resolve(run()).catch(() => undefined);
  } catch {
    return Promise.resolve();
  }
}

/**
 * A notice for a folder that is refreshing marks it dirty and requeues it when
 * the refresh ends, since that refresh may have listed it before the change.
 */
export function createNotifyRefreshes(
  invalidate: WorkspaceNotifyHooks["invalidate"],
  live: () => boolean,
  maxInFlight: number,
  publish: (notice: WorkspaceNotifyNotice) => void = () => undefined,
) {
  const running = new Map<string, { dirty: ListingRefresh | null }>();
  const queued = new Map<string, ListingRefresh>();
  const throttled = new Map<string, WorkspaceNotifyNotice | null>();
  const timers = new Set<ReturnType<typeof setTimeout>>();

  const resyncRoot = (notice: WorkspaceNotifyNotice) => {
    const { rootId } = notice;
    for (const [key, refresh] of queued) {
      if (refresh.listing.rootId === rootId) queued.delete(key);
    }
    const resync = {
      ...notice,
      relativePath: LOST_NOTICES_PATH,
      kind: "resync",
    };
    // A lost-notices resync must cover Git too, even if ignored output was
    // the notice that exceeded a queue bound.
    delete resync.gitRelevant;
    publish(resync);
    void settled(() => invalidate(resync, { rootId, relativePath: "" }));
  };

  const start = (key: string, refresh: ListingRefresh) => {
    const entry = { dirty: null as ListingRefresh | null };
    running.set(key, entry);
    void settled(() => invalidate(refresh.notice, refresh.listing)).finally(
      () => {
        running.delete(key);
        if (!live()) return;
        // It held a slot, so it may join a full queue: at most maxInFlight
        // entries above the bound.
        if (entry.dirty) queued.set(key, entry.dirty);
        const next = queued.entries().next();
        if (!next.done) {
          queued.delete(next.value[0]);
          start(...next.value);
        }
      },
    );
  };

  const folder = (refresh: ListingRefresh) => {
    const key = listingKey(refresh.listing);
    const current = running.get(key);
    const waiting = queued.has(key);
    if (current) {
      current.dirty = refresh;
    } else if (!waiting && running.size < maxInFlight) {
      start(key, refresh);
    } else if (waiting || queued.size < MAX_QUEUED_NOTIFY_INVALIDATIONS) {
      queued.set(key, refresh);
    } else {
      resyncRoot(refresh.notice);
    }
  };

  const ignoredFolder = (notice: WorkspaceNotifyNotice) => {
    const key = listingKey(notice);
    if (throttled.has(key)) {
      const pending = throttled.get(key);
      // An untracked write later in the window must not hide a tracked edit.
      throttled.set(
        key,
        pending && pending.gitRelevant !== false ? pending : notice,
      );
      return;
    }
    if (throttled.size >= MAX_THROTTLED_IGNORED_FOLDERS) {
      resyncRoot(notice);
      return;
    }
    throttled.set(key, null);
    const { rootId, relativePath } = notice;
    publish(notice);
    folder({ notice, listing: { rootId, relativePath } });
    const timer = setTimeout(() => {
      timers.delete(timer);
      const next = throttled.get(key);
      throttled.delete(key);
      if (next && live()) ignoredFolder(next);
    }, IGNORED_SUBTREE_REFRESH_MS);
    timers.add(timer);
  };

  const dispose = () => {
    for (const timer of timers) clearTimeout(timer);
    timers.clear();
    throttled.clear();
    queued.clear();
  };

  return { folder, ignoredFolder, dispose };
}
