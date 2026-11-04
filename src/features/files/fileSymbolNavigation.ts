import { useWorkbenchStore } from "../../app/store";
import { sameResource } from "../../domain/resourceIdentity";
import type { ResourceRef } from "../../domain/workbench";
import {
  findLocalSymbol,
  type OutlineNavigationSelection,
} from "../outline/outlineNavigation";
import type { EditorAdapterHandle } from "./EditorAdapter";
import { contentText, sameContent } from "./editorContent";
import type { ActiveEditorState } from "./fileEditorController";

/** Navigate only if the requested File and buffer survive syntax loading. */
export async function navigateFileSymbol(
  input: EditorAdapterHandle | null,
  resource: ResourceRef | undefined,
  editor: ActiveEditorState | null,
  selection: OutlineNavigationSelection | undefined,
): Promise<string | null> {
  if (!input || !resource || !selection || editor?.phase !== "ready") {
    return "Local symbol navigation is unavailable.";
  }
  const { content } = editor;
  const target = await findLocalSymbol(
    resource.relativePath,
    contentText(content),
    selection,
  );
  const current = useWorkbenchStore.getState().activeEditor;
  if (
    current?.phase !== "ready" ||
    !sameResource(current.resource, resource) ||
    !sameContent(current.content, content)
  ) {
    return null;
  }
  if (!target) return "No declaration in this File for that name.";
  input.focus();
  input.setSelection(target.start, target.end);
  return "Moved to local symbol.";
}
