import { sameResource } from "./resourceIdentity";
import type { ResourceRef } from "./resource";

export { createInjectedEditorWatchDelivery } from "./editorWatchDelivery";

export const EDITOR_EXTERNAL_WATCH_EVENT = "editor-external-watch-v1";

export type EditorExternalWatchKind = "changed" | "deleted";

export interface EditorExternalWatchNotice {
  resource: ResourceRef;
  generation: number;
  kind: EditorExternalWatchKind;
}

export interface EditorWatchDelivery {
  subscribe(listener: (notice: EditorExternalWatchNotice) => void): () => void;
}

export interface EditorWatchSubscription {
  resource: ResourceRef;
  cancel(): Promise<void>;
  delivery: EditorWatchDelivery;
}

export function noticeMatchesWatch(
  notice: EditorExternalWatchNotice,
  resource: ResourceRef,
): boolean {
  return sameResource(notice.resource, resource);
}
