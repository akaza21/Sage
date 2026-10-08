import {
  Braces,
  FileCode2,
  FileImage,
  FileJson,
  FileLock,
  FileSpreadsheet,
  FileText,
  Folder,
  Link2,
  NotebookText,
} from "../../ui/leanIcons";

import type { FileKind } from "./fileKind";
import { fileKindForPath } from "./fileKind";

const ICONS: Record<FileKind, typeof FileCode2> = {
  folder: Folder,
  symlink: Link2,
  typescript: FileCode2,
  javascript: FileCode2,
  python: FileCode2,
  rust: FileCode2,
  markdown: FileText,
  json: FileJson,
  css: Braces,
  html: FileCode2,
  lock: FileLock,
  image: FileImage,
  notebook: NotebookText,
  table: FileSpreadsheet,
  toml: FileText,
  plain: FileCode2,
};

export function FileKindIcon({
  path,
  entryKind = "file",
  size = 14,
}: {
  path: string;
  entryKind?: "file" | "directory" | "symlink";
  size?: number;
}) {
  const kind = fileKindForPath(path, entryKind);
  const Icon = ICONS[kind];
  return (
    <Icon
      className={`file-kind-icon file-kind-icon--${kind}`}
      size={size}
      strokeWidth={1.7}
      aria-hidden="true"
    />
  );
}
