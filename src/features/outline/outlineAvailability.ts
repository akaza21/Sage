import { editorLanguageForPath } from "../files/editorLanguage";

export const MAX_OUTLINE_SOURCE_CHARS = 1024 * 1024;

export type OutlineAvailability = "available" | "unsupported" | "too-large";

export function outlineAvailability(
  path: string,
  length: number,
): OutlineAvailability {
  const language = editorLanguageForPath(path);
  if (language === "plain" || language === "html") return "unsupported";
  return length > MAX_OUTLINE_SOURCE_CHARS ? "too-large" : "available";
}
