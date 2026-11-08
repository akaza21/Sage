import type { EditorDocument, ResourceRef } from "../../domain/workbench";
import type { EditorPort } from "../../ports/workbenchPorts";
import { contentText, type EditorContent } from "./editorContent";

export interface UnsavedBuffer {
  resource: ResourceRef;
  content: EditorContent;
  lineEnding: EditorDocument["lineEnding"];
  revision: number;
}

/**
 * Puts an unsaved buffer on top of the File as it is on disk now, so the next
 * save writes the buffer over the change made outside Sage ("Keep mine").
 *
 * A native buffer remembers the disk version it was opened from and a save
 * refuses once disk has moved on. Dropping that buffer and snapshotting the
 * same text against the current version is the overwrite, built from the
 * existing commands. Between the two calls the text lives only in the
 * WebView, as it does before any snapshot of a fresh edit.
 */
export async function rebaseBufferOnDisk(
  editor: Pick<
    EditorPort,
    "discardEditorBuffer" | "snapshotEditorBuffer" | "openEditorDocument"
  >,
  buffer: UnsavedBuffer,
): Promise<EditorDocument> {
  const disk = await editor.discardEditorBuffer(buffer.resource);
  await editor.snapshotEditorBuffer({
    resource: buffer.resource,
    base: disk.base,
    lineEnding: buffer.lineEnding,
    content: contentText(buffer.content),
    revision: Math.max(1, buffer.revision),
  });
  return editor.openEditorDocument(buffer.resource);
}
