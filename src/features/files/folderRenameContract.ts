import { type ResourceRef } from "../../domain/workbench";
import { renameTargetResource } from "./fileRenameContract";

export function renameFolderTargetResource(
  source: ResourceRef,
  basename: string,
): ResourceRef {
  return renameTargetResource(source, basename);
}
