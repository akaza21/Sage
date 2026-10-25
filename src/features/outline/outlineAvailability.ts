import { editorLanguageForPath } from "../files/editorLanguage";

/** The editable File limit; larger documents get no Outline, not a partial one. */
export const MAX_OUTLINE_SOURCE_CHARS = 1024 * 1024;

export type OutlineAvailability = "available" | "unsupported" | "too-large";

/**
 * Whether a File can have an Outline, decided without loading a grammar, so
 * the panel can say why it is empty instead of showing a bare "no symbols".
 */
export function outlineAvailability(
  path: string,
  length: number,
): OutlineAvailability {
  const language = editorLanguageForPath(path);
  if (language === "plain" || language === "html") return "unsupported";
  return length > MAX_OUTLINE_SOURCE_CHARS ? "too-large" : "available";
}
