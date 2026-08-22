import {
  createInjectedEditorWatchDelivery,
  noticeMatchesWatch,
  type EditorExternalWatchNotice,
  type EditorWatchSubscription,
} from "../domain/editorWatch";
import type { ResourceRef } from "../domain/workbench";

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
