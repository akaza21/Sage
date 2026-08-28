import {
  editorLanguageForPath,
  MAX_EDITOR_LANGUAGE_PATH_LENGTH,
} from "./editorLanguage";

export type FileKind =
  | "folder"
  | "symlink"
  | "typescript"
  | "javascript"
  | "python"
  | "rust"
  | "markdown"
  | "json"
  | "css"
  | "html"
  | "lock"
  | "image"
  | "notebook"
  | "table"
  | "toml"
  | "plain";

const NAME_KINDS: Readonly<Record<string, FileKind>> = {
  "uv.lock": "lock",
  "package-lock.json": "lock",
  "cargo.lock": "lock",
  "pnpm-lock.yaml": "lock",
  "yarn.lock": "lock",
};

const EXTENSION_KINDS: Readonly<Record<string, FileKind>> = {
  lock: "lock",
  png: "image",
  jpg: "image",
  jpeg: "image",
  gif: "image",
  webp: "image",
  svg: "image",
  ipynb: "notebook",
  csv: "table",
  tsv: "table",
  toml: "toml",
};

export function fileKindForPath(
  path: string,
  entryKind: "file" | "directory" | "symlink" = "file",
): FileKind {
  if (entryKind === "directory") return "folder";
  if (entryKind === "symlink") return "symlink";
  if (path.length === 0 || path.length > MAX_EDITOR_LANGUAGE_PATH_LENGTH) {
    return "plain";
  }
  const name = (path.split(/[\\/]/).at(-1) ?? "").toLowerCase();
  if (NAME_KINDS[name]) return NAME_KINDS[name]!;
  const language = editorLanguageForPath(path);
  if (language.startsWith("typescript")) return "typescript";
  if (language.startsWith("javascript")) return "javascript";
  if (language === "python") return "python";
  if (language === "rust") return "rust";
  if (language === "markdown") return "markdown";
  if (language === "json") return "json";
  if (language === "css") return "css";
  if (language === "html") return "html";
  const separator = name.lastIndexOf(".");
  if (separator <= 0 || separator === name.length - 1) return "plain";
  return EXTENSION_KINDS[name.slice(separator + 1)] ?? "plain";
}
