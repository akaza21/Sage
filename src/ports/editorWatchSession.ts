import {
  noticeMatchesWatch,
  type EditorExternalWatchNotice,
  type EditorWatchSubscription,
} from "../domain/editorWatch";
import { createInjectedEditorWatchDelivery } from "../domain/editorWatchDelivery";
import type { ResourceRef } from "../domain/workbench";
import { EDITOR_EXTERNAL_WATCH_EVENT } from "../domain/editorWatch";
import { invokeNative, isTauriRuntime } from "./nativeInvoke";

export interface EditorWatchNativeHooks {
  listen(
    resource: ResourceRef,
    emit: (notice: EditorExternalWatchNotice) => void,
  ): Promise<() => void>;
  watch(resource: ResourceRef): Promise<void>;
  unwatch(resource: ResourceRef): Promise<void>;
}

export async function startEditorWatchSession(
  resource: ResourceRef,
  hooks: EditorWatchNativeHooks,
): Promise<EditorWatchSubscription> {
  const injected = createInjectedEditorWatchDelivery();
  const unlisten = await hooks.listen(resource, (notice) => {
    if (noticeMatchesWatch(notice, resource)) {
      injected.emit(notice);
    }
  });
  try {
    await hooks.watch(resource);
  } catch (error) {
    unlisten();
    injected.close();
    throw error;
  }
  return {
    resource,
    delivery: injected.delivery,
    cancel: async () => {
      injected.close();
      unlisten();
      await hooks.unwatch(resource);
    },
  };
}

export function startNativeEditorWatchSession(
  resource: ResourceRef,
): Promise<EditorWatchSubscription> {
  return startEditorWatchSession(resource, {
    async listen(_watched, emit) {
      if (!isTauriRuntime()) return () => undefined;
      const { listen } = await import("@tauri-apps/api/event");
      return listen<EditorExternalWatchNotice>(
        EDITOR_EXTERNAL_WATCH_EVENT,
        (event) => emit(event.payload),
      );
    },
    watch(watched) {
      return invokeNative("watch_editor_document", { resource: watched });
    },
    unwatch(watched) {
      return invokeNative("unwatch_editor_document", { resource: watched });
    },
  });
}
