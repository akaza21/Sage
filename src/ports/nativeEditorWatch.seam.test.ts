import { emit } from "@tauri-apps/api/event";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, expect, it } from "vitest";

import {
  EDITOR_EXTERNAL_WATCH_EVENT,
  type EditorExternalWatchNotice,
} from "../domain/editorWatch";
import { createNativeWorkbenchPorts } from "./nativeWorkbenchPorts";

afterEach(() => clearMocks());

it("delivers only the watched resource's notices and stops delivery after cancellation", async () => {
  mockIPC(() => undefined, { shouldMockEvents: true });
  const resource = { rootId: "root-notes", relativePath: "journal/today.md" };
  const subscription =
    await createNativeWorkbenchPorts().editor.watchEditorDocument(resource);
  const received: EditorExternalWatchNotice[] = [];
  subscription.delivery.subscribe((notice) => received.push(notice));
  const notice: EditorExternalWatchNotice = {
    resource,
    generation: 1,
    kind: "changed",
  };

  await emit(EDITOR_EXTERNAL_WATCH_EVENT, {
    ...notice,
    resource: { ...resource, relativePath: "journal/other.md" },
  });
  await emit(EDITOR_EXTERNAL_WATCH_EVENT, notice);
  expect(received).toEqual([notice]);

  await subscription.cancel();
  await emit(EDITOR_EXTERNAL_WATCH_EVENT, { ...notice, generation: 2 });
  expect(received).toEqual([notice]);
});
