import { afterEach, describe, expect, it, vi } from "vitest";

import { childResource, parentResource } from "../../domain/resourceIdentity";
import {
  MAX_IN_FLIGHT_NOTIFY_INVALIDATIONS,
  MAX_NOTIFY_SEARCH_SUBSCRIBERS,
  acceptWorkspaceNotifyNotice,
  listingResourceForNotify,
  publishWorkspaceNotifySearch,
  resetWorkspaceNotifyComposeForTests,
  subscribeWorkspaceNotifySearch,
  syncLiveProjectWorkspaceNotify,
  type LiveProjectNotifyStamp,
  type WorkspaceNotifyNotice,
} from "./workspaceNotifyCompose";

const live: LiveProjectNotifyStamp = {
  projectId: "project-a",
  projectGeneration: 3,
  transitionEpoch: 7,
  roots: [{ rootId: "root-a" }],
};

const notice: WorkspaceNotifyNotice = {
  projectId: "project-a",
  projectGeneration: 3,
  transitionEpoch: 7,
  rootId: "root-a",
  relativePath: "src/main.ts",
  kind: "changed",
};

describe("workspace notify compose", () => {
  afterEach(async () => {
    resetWorkspaceNotifyComposeForTests();
    await syncLiveProjectWorkspaceNotify(null, {
      watch: vi.fn(),
      unwatch: vi.fn(),
      listen: vi.fn(async () => () => undefined),
      invalidate: vi.fn(),
    });
  });

  it("accepts a notice that matches the live Project stamp", () => {
    expect(acceptWorkspaceNotifyNotice(notice, live)).toEqual(notice);
  });

  it("accepts a resync notice and maps it to the root listing", () => {
    const resync = { ...notice, relativePath: "lost-notices", kind: "resync" };
    expect(acceptWorkspaceNotifyNotice(resync, live)).toEqual(resync);
    expect(listingResourceForNotify(resync)).toEqual(childResource("root-a"));
  });

  it("rejects a notice whose path or kind is outside the native bounds", () => {
    const oversize = "a".repeat(16 * 1024 + 1);
    expect(
      acceptWorkspaceNotifyNotice(
        { ...notice, relativePath: "src/../x.ts" },
        live,
      ),
    ).toBeNull();
    expect(
      acceptWorkspaceNotifyNotice(
        { ...notice, relativePath: "src\\main.ts" },
        live,
      ),
    ).toBeNull();
    expect(
      acceptWorkspaceNotifyNotice(
        { ...notice, relativePath: "/src/main.ts" },
        live,
      ),
    ).toBeNull();
    expect(
      acceptWorkspaceNotifyNotice({ ...notice, relativePath: "src/" }, live),
    ).toBeNull();
    expect(
      acceptWorkspaceNotifyNotice({ ...notice, relativePath: oversize }, live),
    ).toBeNull();
    expect(
      acceptWorkspaceNotifyNotice({ ...notice, kind: "renamed" }, live),
    ).toBeNull();
  });

  it("ignores a stale projectGeneration or transitionEpoch", () => {
    expect(
      acceptWorkspaceNotifyNotice({ ...notice, projectGeneration: 4 }, live),
    ).toBeNull();
    expect(
      acceptWorkspaceNotifyNotice({ ...notice, transitionEpoch: 8 }, live),
    ).toBeNull();
    expect(
      acceptWorkspaceNotifyNotice({ ...notice, projectId: "project-b" }, live),
    ).toBeNull();
  });

  it("maps the changed path to its parent listing resource", () => {
    expect(listingResourceForNotify(notice)).toEqual(
      parentResource({ rootId: "root-a", relativePath: "src/main.ts" }),
    );
    expect(
      listingResourceForNotify({ ...notice, relativePath: "notes.md" }),
    ).toEqual(childResource("root-a"));
  });

  it("watches the live Project and invalidates only accepted notices", async () => {
    const watch = vi.fn().mockResolvedValue({ generation: 2 });
    const unwatch = vi.fn().mockResolvedValue(undefined);
    let emit: (value: WorkspaceNotifyNotice) => void = () => undefined;
    const listen = vi.fn(async (next: typeof emit) => {
      emit = next;
      return () => undefined;
    });
    const invalidate = vi.fn();

    await syncLiveProjectWorkspaceNotify(live, {
      watch,
      unwatch,
      listen,
      invalidate,
    });

    expect(watch).toHaveBeenCalledWith({
      stamp: {
        projectId: "project-a",
        projectGeneration: 3,
        transitionEpoch: 7,
      },
      roots: [{ rootId: "root-a" }],
    });

    emit(notice);
    emit({ ...notice, projectGeneration: 9 });
    expect(invalidate).toHaveBeenCalledTimes(1);
    expect(invalidate).toHaveBeenCalledWith(
      notice,
      parentResource({ rootId: "root-a", relativePath: "src/main.ts" }),
    );
  });

  it("unwatches the previous Project when the live Project is cleared", async () => {
    const watch = vi.fn().mockResolvedValue({ generation: 1 });
    const unwatch = vi.fn().mockResolvedValue(undefined);
    const listen = vi.fn(async () => () => undefined);

    await syncLiveProjectWorkspaceNotify(live, {
      watch,
      unwatch,
      listen,
      invalidate: vi.fn(),
    });
    await syncLiveProjectWorkspaceNotify(null, {
      watch,
      unwatch,
      listen,
      invalidate: vi.fn(),
    });

    expect(unwatch).toHaveBeenCalledWith({ projectId: "project-a" });
  });

  it("listens before watch so queued notices are not dropped", async () => {
    const order: string[] = [];
    const invalidate = vi.fn();
    let emit: (value: WorkspaceNotifyNotice) => void = () => undefined;
    await syncLiveProjectWorkspaceNotify(live, {
      async watch() {
        order.push("watch");
        emit(notice);
        return { generation: 1 };
      },
      unwatch: vi.fn(),
      async listen(next) {
        order.push("listen");
        emit = next;
        return () => undefined;
      },
      invalidate,
    });
    expect(order).toEqual(["listen", "watch"]);
    expect(invalidate).toHaveBeenCalledTimes(1);
  });

  it("coalesces a second notice for a listing that is already refreshing", async () => {
    let emit: (value: WorkspaceNotifyNotice) => void = () => undefined;
    let finish: () => void = () => undefined;
    const invalidate = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    );
    await syncLiveProjectWorkspaceNotify(live, {
      watch: vi.fn().mockResolvedValue({ generation: 1 }),
      unwatch: vi.fn(),
      async listen(next) {
        emit = next;
        return () => undefined;
      },
      invalidate,
    });
    emit(notice);
    emit(notice);
    expect(invalidate).toHaveBeenCalledTimes(1);
    finish();
  });

  it("drops a ninth distinct in-flight listing refresh", async () => {
    expect(MAX_IN_FLIGHT_NOTIFY_INVALIDATIONS).toBe(8);
    let emit: (value: WorkspaceNotifyNotice) => void = () => undefined;
    const invalidate = vi.fn(() => new Promise<void>(() => undefined));
    await syncLiveProjectWorkspaceNotify(live, {
      watch: vi.fn().mockResolvedValue({ generation: 1 }),
      unwatch: vi.fn(),
      async listen(next) {
        emit = next;
        return () => undefined;
      },
      invalidate,
    });
    for (let index = 0; index < 9; index += 1) {
      emit({ ...notice, relativePath: `dir-${index}/file.ts` });
    }
    expect(invalidate).toHaveBeenCalledTimes(8);
  });

  it("ignores a ninth Search subscriber", () => {
    expect(MAX_NOTIFY_SEARCH_SUBSCRIBERS).toBe(8);
    const heard: number[] = [];
    const unsubscribers: Array<() => void> = [];
    for (let index = 0; index < 9; index += 1) {
      const slot = index;
      unsubscribers.push(
        subscribeWorkspaceNotifySearch(() => {
          heard.push(slot);
        }),
      );
    }
    publishWorkspaceNotifySearch(notice);
    expect(heard).toEqual([0, 1, 2, 3, 4, 5, 6, 7]);
    for (const unsubscribe of unsubscribers) {
      unsubscribe();
    }
  });
});
