import { sameResource } from "../../domain/resourceIdentity";
import type { ResourceRef } from "../../domain/workbench";

/**
 * A one-shot request to focus one File's editor, for flows that open a File
 * and expect typing to go there (New File > Create). Only that File's surface
 * takes it, whether it renders before or after the request is made.
 */
// A request no editor takes soon (the File failed to open) expires, so it
// cannot steal focus later.
const REQUEST_LIFETIME_MS = 5_000;
let request: { resource: ResourceRef; at: number } | null = null;
const listeners = new Set<() => void>();

export function requestEditorFocus(resource: ResourceRef): void {
  request = { resource, at: Date.now() };
  for (const listener of listeners) listener();
}

export function takeEditorFocusRequest(resource: ResourceRef): boolean {
  if (!request || !sameResource(request.resource, resource)) return false;
  const taken = Date.now() - request.at < REQUEST_LIFETIME_MS;
  request = null;
  return taken;
}

export function subscribeEditorFocusRequest(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}
