/**
 * A request, sent as the detail of a `sage:editor-text-replace` window
 * event, that the File editor showing `resourceKey` change its text from
 * `expected` to `content` as one undoable edit. The editor sets `handled`
 * when it shows that File, and `applied` when it made the change; a text
 * other than `expected` is left as it is.
 */
export interface EditorTextReplace {
  resourceKey: string;
  expected: string;
  content: string;
  handled: boolean;
  applied: boolean;
}
