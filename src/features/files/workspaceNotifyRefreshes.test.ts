import { afterEach, describe, expect, it, vi } from "vitest";

import { childResource } from "../../domain/resourceIdentity";
import type { ResourceRef } from "../../domain/workbench";
import type { WorkspaceNotifyNotice } from "./workspaceNotifyCompose";
import {
  IGNORED_SUBTREE_REFRESH_MS,
  MAX_QUEUED_NOTIFY_INVALIDATIONS,
  createNotifyRefreshes,
} from "./workspaceNotifyRefreshes";

const MAX_IN_FLIGHT = 2;

const notice = (
  relativePath: string,
  kind = "changed",
  rootId = "root-a",
): WorkspaceNotifyNotice => ({
  projectId: "project-a",
  projectGeneration: 3,
  transitionEpoch: 7,
  rootId,
  relativePath,
  kind,
});

const folder = (relativePath: string, rootId = "root-a") => ({
  notice: notice(`${relativePath}/file.ts`, "changed", rootId),
  listing: childResource(rootId, relativePath),
});

/** Refreshes that end only when the test says so, in the order started. */
function heldRefreshes() {
  const started: Array<{
    notice: WorkspaceNotifyNotice;
    listing: ResourceRef;
    finish: () => void;
  }> = [];
  const invalidate = vi.fn(
    (notice: WorkspaceNotifyNotice, listing: ResourceRef) =>
      new Promise<void>((resolve) => {
        started.push({ notice, listing, finish: resolve });
      }),
  );
  const paths = () => started.map((entry) => entry.listing.relativePath);
  return { started, invalidate, paths };
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

describe("workspace notify refreshes", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("re-runs a folder changed mid-refresh after the folders already waiting", async () => {
    const { started, invalidate, paths } = heldRefreshes();
    const refreshes = createNotifyRefreshes(
      invalidate,
      () => true,
      MAX_IN_FLIGHT,
    );
    refreshes.folder(folder("a"));
    refreshes.folder(folder("b"));
    refreshes.folder(folder("c"));
    refreshes.folder(folder("a"));
    expect(paths()).toEqual(["a", "b"]);

    started[0].finish();
    await settle();
    expect(paths()).toEqual(["a", "b", "c"]);
    started[1].finish();
    await settle();
    expect(paths()).toEqual(["a", "b", "c", "a"]);
    started[2].finish();
    started[3].finish();
    await settle();
    expect(paths()).toHaveLength(4);
  });

  it("resyncs a root once when its folders overflow the queue", async () => {
    const { invalidate, paths } = heldRefreshes();
    const refreshes = createNotifyRefreshes(
      invalidate,
      () => true,
      MAX_IN_FLIGHT,
    );
    const folders = MAX_IN_FLIGHT + MAX_QUEUED_NOTIFY_INVALIDATIONS;
    for (let index = 0; index < folders + 20; index += 1) {
      refreshes.folder(folder(`dir-${index}`));
    }
    expect(paths()).toEqual(["dir-0", "dir-1", ""]);
    expect(invalidate).toHaveBeenLastCalledWith(
      notice("lost-notices", "resync"),
      childResource("root-a"),
    );
  });

  it("passes on a resync requested while another runs", async () => {
    const { invalidate } = heldRefreshes();
    const refreshes = createNotifyRefreshes(
      invalidate,
      () => true,
      MAX_IN_FLIGHT,
    );
    const overflow = (wave: string) => {
      for (
        let index = 0;
        index <= MAX_QUEUED_NOTIFY_INVALIDATIONS;
        index += 1
      ) {
        refreshes.folder(folder(`${wave}-${index}`));
      }
    };
    refreshes.folder(folder("a"));
    refreshes.folder(folder("b"));
    overflow("first");
    overflow("second");
    const resyncs = invalidate.mock.calls.filter(
      ([sent]) => sent.kind === "resync",
    );
    expect(resyncs).toHaveLength(2);
  });

  it("starts nothing more once the session is gone", async () => {
    const { started, invalidate, paths } = heldRefreshes();
    let live = true;
    const refreshes = createNotifyRefreshes(
      invalidate,
      () => live,
      MAX_IN_FLIGHT,
    );
    for (const name of ["a", "b", "c"]) refreshes.folder(folder(name));
    refreshes.folder(folder("a"));
    live = false;
    started[0].finish();
    await settle();
    expect(paths()).toEqual(["a", "b"]);
  });

  it("refreshes an ignored folder at once, then at most once per interval", async () => {
    vi.useFakeTimers();
    const invalidate = vi.fn();
    const refreshes = createNotifyRefreshes(
      invalidate,
      () => true,
      MAX_IN_FLIGHT,
    );
    for (let index = 0; index < 50; index += 1) {
      refreshes.ignoredFolder(notice("target", "ignored"));
    }
    expect(invalidate).toHaveBeenCalledTimes(1);
    expect(invalidate).toHaveBeenCalledWith(
      notice("target", "ignored"),
      childResource("root-a", "target"),
    );
    await vi.advanceTimersByTimeAsync(IGNORED_SUBTREE_REFRESH_MS - 1);
    expect(invalidate).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(invalidate).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(IGNORED_SUBTREE_REFRESH_MS * 5);
    expect(invalidate).toHaveBeenCalledTimes(2);

    refreshes.ignoredFolder(notice("dist", "ignored"));
    expect(invalidate).toHaveBeenCalledTimes(3);
  });

  it("drops pending ignored-folder refreshes when disposed", async () => {
    vi.useFakeTimers();
    const invalidate = vi.fn();
    const refreshes = createNotifyRefreshes(
      invalidate,
      () => true,
      MAX_IN_FLIGHT,
    );
    refreshes.ignoredFolder(notice("target", "ignored"));
    refreshes.ignoredFolder(notice("target", "ignored"));
    refreshes.dispose();
    expect(vi.getTimerCount()).toBe(0);
    await vi.advanceTimersByTimeAsync(IGNORED_SUBTREE_REFRESH_MS * 3);
    expect(invalidate).toHaveBeenCalledTimes(1);
  });
});
