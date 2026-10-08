import type { RefObject } from "react";
import type { ResourceRef } from "../../domain/workbench";
import { RenameResourceControl } from "./RenameResourceControl";

export function RenameFolderControl(props: {
  source: ResourceRef | null;
  rootName: string | null;
  returnFocusRef: RefObject<HTMLButtonElement | null>;
  onDialogOpen: () => void;
}) {
  return <RenameResourceControl {...props} kind="Folder" />;
}
