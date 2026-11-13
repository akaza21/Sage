import type { ResourceRef } from "../../domain/workbench";
import type {
  WorkspaceNotifyHooks,
  WorkspaceNotifyNotice,
} from "./workspaceNotifyCompose";

/** Folder refreshes waiting for an in-flight slot; past this, the root resyncs. */
export const MAX_QUEUED_NOTIFY_INVALIDATIONS = 64;
/**
 * The shortest gap between two invalidations of one ignored folder. A
 * `cargo build` with `target` expanded changes it many times a second; once a
 * second keeps the listing current without reloading it for every write.
 */
export const IGNORED_SUBTREE_REFRESH_MS = 1_000;
/** Ignored folders throttled at once; past this, the root resyncs. */
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
 * Schedules the listing refreshes of one bound session, each bounded.
 *
 * - `folder`: at most `maxInFlight` folders refresh at once, one refresh per
 *   folder. A notice for a folder that is refreshing marks it dirty: when the
 *   refresh ends the folder joins the back of the queue once more, since that
 *   refresh may have listed it before the change, and waits its turn behind
 *   folders already waiting. Other folders wait in a queue of
 *   MAX_QUEUED_NOTIFY_INVALIDATIONS; past that the root resyncs, which
 *   covers that root's waiting folders, so they leave the queue. Every
 *   resync is passed on, even while one runs: the listing side coalesces
 *   them and runs once more after a resync that a change overtook.
 * - `ignoredFolder`: publishes subtree notices at once, then at most every
 *   IGNORED_SUBTREE_REFRESH_MS while they keep coming, with one last notice
 *   after the final change. Listing refreshes use the folder queue, retaining
 *   changes until an in-flight read ends. Publishing does not wait for reads.
 *   Each listing load still takes one of the shared listing slots.
 *
 * Nothing scheduled starts after `live()` turns false, and `dispose` drops
 * the throttle timers when the session ends.
 */
export function createNotifyRefreshes(
  invalidate: WorkspaceNotifyHooks["invalidate"],
  live: () => boolean,
  maxInFlight: number,
  publish: (notice: WorkspaceNotifyNotice) => void = () => undefined,
) {
  const running = new Map<string, { dirty: ListingRefresh | null }>();
  const queued = new Map<string, ListingRefresh>();
  // Each throttled ignored folder, with the notice that came in since its
  // last refresh, if any.
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
        // One slot freed: the longest-waiting folder takes it.
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
      throttled.set(key, notice);
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
