import type { Text } from "@codemirror/state";

// Mirrors sage_workspace::MAX_EDITABLE_BYTES.
export const MAX_EDITABLE_CONTENT_BYTES = 1024 * 1024;
export const MAX_FIND_QUERY_LENGTH = 256;

/**
 * File text as the store holds it: the string read from disk, or, once the
 * user edits, the editor's own immutable document. Keeping the document lets a
 * keystroke reach the store without building the whole File as a string.
 * Only the type is imported, so CodeMirror stays out of the core bundle.
 */
export type EditorContent = string | Text;

const strings = new WeakMap<Text, string>();

/** The content as a string, built at most once per editor document. */
export function contentText(content: EditorContent): string {
  if (typeof content === "string") return content;
  let text = strings.get(content);
  if (text === undefined) {
    text = content.toString();
    strings.set(content, text);
  }
  return text;
}

/** Whether two contents hold the same text, without stringifying when lengths differ. */
export function sameContent(left: EditorContent, right: EditorContent) {
  if (left === right) return true;
  if (left.length !== right.length) return false;
  if (typeof left !== "string" && typeof right !== "string") {
    return left.eq(right);
  }
  return contentText(left) === contentText(right);
}

/**
 * What the editor knows about an edited document against the last string it
 * was given: that string, and whether the document still equals it. The
 * runtime answers this from shared document structure, in time proportional
 * to the edit.
 */
export interface EditorContentBase {
  content: string;
  unchanged: boolean;
}
