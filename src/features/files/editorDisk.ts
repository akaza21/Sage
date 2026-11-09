import { ABSENT_FINGERPRINT } from "../../domain/workbench";
import type { ReadyEditorState } from "./fileEditorController";

/** The File is gone from disk, whether or not the user kept the buffer. */
export function isDeletedOnDisk(editor: ReadyEditorState): boolean {
  return (
    editor.document.conflict?.kind === "deleted" ||
    editor.document.disk.fingerprint === ABSENT_FINGERPRINT
  );
}
