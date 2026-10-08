import type {
  RenameProjectFolderResult,
  ResourceRef,
} from "../../domain/workbench";
import {
  renameTargetResource,
  validateResourceRenameResult,
  type RenameExpectation,
} from "./fileRenameContract";

export function renameFolderTargetResource(
  source: ResourceRef,
  basename: string,
): ResourceRef {
  return renameTargetResource(source, basename);
}

export function validateFolderRenameResult(
  result: RenameProjectFolderResult,
  expected: RenameExpectation,
): boolean {
  return validateResourceRenameResult(result, expected, "directory");
}
