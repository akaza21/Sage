import { describe, expect, it, vi } from "vitest";

import { startEditorWatchSession } from "./editorWatchSession";

const resource = { rootId: "root-notes", relativePath: "journal/today.md" };

describe("IDE-2 native watch session order", () => {
  it("attaches the listener before starting the native watch", async () => {
    const order: string[] = [];
    await startEditorWatchSession(resource, {
      listen: async () => {
        order.push("listen");
        return () => {
          order.push("unlisten");
        };
      },
      watch: async () => {
        order.push("watch");
      },
      unwatch: async () => {
        order.push("unwatch");
      },
    });
    expect(order).toEqual(["listen", "watch"]);
  });

  it("does not start native watch if listen fails", async () => {
    const watch = vi.fn();
    await expect(
      startEditorWatchSession(resource, {
        listen: async () => {
          throw new Error("listen failed");
        },
        watch,
        unwatch: vi.fn(),
      }),
    ).rejects.toThrow("listen failed");
    expect(watch).not.toHaveBeenCalled();
  });

  it("unlistens when native watch fails after listen", async () => {
    const order: string[] = [];
    await expect(
      startEditorWatchSession(resource, {
        listen: async () => {
          order.push("listen");
          return () => {
            order.push("unlisten");
          };
        },
        watch: async () => {
          order.push("watch");
          throw new Error("watch failed");
        },
        unwatch: async () => {
          order.push("unwatch");
        },
      }),
    ).rejects.toThrow("watch failed");
    expect(order).toEqual(["listen", "watch", "unlisten"]);
  });
});
