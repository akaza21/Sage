import type {
  RenameProjectFileRequest,
  TrashProjectResourceRequest,
} from "../domain/workbench";

export class BrowserFileRenameUnavailableError extends Error {
  override name = "BrowserFileRenameUnavailableError";
}

export class BrowserResourceTrashUnavailableError extends Error {
  override name = "BrowserResourceTrashUnavailableError";
}

export async function renameBrowserProjectFile(
  _request: RenameProjectFileRequest,
): Promise<never> {
  throw new BrowserFileRenameUnavailableError(
    "File rename requires the native Sage desktop",
  );
}

export async function trashBrowserProjectResource(
  _request: TrashProjectResourceRequest,
): Promise<never> {
  throw new BrowserResourceTrashUnavailableError(
    "Operating-system trash requires the native Sage desktop",
  );
}
