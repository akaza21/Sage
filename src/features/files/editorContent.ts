import type { Text } from "@codemirror/state";

// Mirrors editor_content::MAX_EDITABLE_BYTES.
export const MAX_EDITABLE_CONTENT_BYTES = 1024 * 1024;
export const MAX_FIND_QUERY_LENGTH = 256;

/** Only the type is imported, so CodeMirror stays out of the core bundle. */
export type EditorContent = string | Text;

const strings = new WeakMap<Text, string>();

export function contentText(content: EditorContent): string {
  if (typeof content === "string") return content;
  let text = strings.get(content);
  if (text === undefined) {
    text = content.toString();
    strings.set(content, text);
  }
  return text;
}

export function sameContent(left: EditorContent, right: EditorContent) {
  if (left === right) return true;
  if (left.length !== right.length) return false;
  if (typeof left !== "string" && typeof right !== "string") {
    return left.eq(right);
  }
  return contentText(left) === contentText(right);
}

export interface EditorContentBase {
  content: string;
  unchanged: boolean;
}
