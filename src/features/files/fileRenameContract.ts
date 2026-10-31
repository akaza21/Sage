import { parentResource, sameResource } from "../../domain/resourceIdentity";
import { type ResourceRef } from "../../domain/workbench";
import { newFileResource, validateNewFilePath } from "./newFileContract";

export function renameTargetResource(
  source: ResourceRef,
  basename: string,
): ResourceRef {
  const parent = parentResource(source);
  const validation = validateNewFilePath(parent, basename);
  if (validation) throw new Error(validation.replace(/^File/, "Rename target"));
  const target = newFileResource(parent, basename);
  if (sameResource(source, target)) {
    throw new Error("Rename target must differ from the source.");
  }
  return target;
}

export function renameFolderTargetResource(
  source: ResourceRef,
  basename: string,
): ResourceRef {
  return renameTargetResource(source, basename);
}
