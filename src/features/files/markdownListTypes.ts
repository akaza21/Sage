export interface ListItemShape {
  depth: number;
  ordered: boolean;
  checked?: boolean;
  /** Non-default start of a nested ordered list. */
  start?: number;
  /** A different bullet or ordered delimiter starts another list. */
  newList?: true;
}

export interface MarkdownListBlock {
  kind: "list";
  items: readonly string[];
  ordered?: true;
  start?: number;
  /** Present only when an item is nested or is a task. */
  shape?: readonly ListItemShape[];
}
