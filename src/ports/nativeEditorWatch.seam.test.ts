import { expect, it, vi } from "vitest";
import { createNativeWorkbenchPorts } from "./nativeWorkbenchPorts";

const { order } = vi.hoisted(() => ({ order: [] as string[] }));

vi.mock("./nativeInvoke", () => ({
  isTauriRuntime: () => true,
  invokeNative: vi.fn(async (command: string) => {
    order.push(command);
  }),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => {
    order.push("listen");
    return () => order.push("unlisten");
  }),
}));

it("the native editor port preserves watch ordering and cancellation on first use", async () => {
  const resource = { rootId: "root-notes", relativePath: "journal/today.md" };
  const subscription =
    await createNativeWorkbenchPorts().editor.watchEditorDocument(resource);
  expect(subscription.resource).toEqual(resource);
  expect(order).toEqual(["listen", "watch_editor_document"]);
  await subscription.cancel();
  expect(order).toEqual([
    "listen",
    "watch_editor_document",
    "unlisten",
    "unwatch_editor_document",
  ]);
});
