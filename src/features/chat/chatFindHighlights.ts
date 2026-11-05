import { type ChatMatch } from "./chatFindModel";

type Ranges = { other: Range[]; current: Range[] };

/** At most two chat panes are visible, so at most two owners paint. */
const MAX_HIGHLIGHT_OWNERS = 2;

const owners = new Map<symbol, Ranges>();

/** The rendered blocks, in the order chatTurnText produces segments. */
const SEGMENT_SELECTOR =
  "p, h1, h2, h3, h4, h5, h6, [data-markdown-inline], th, td, pre > code";

function segmentElements(message: Element): Element[] {
  const markdown = message.querySelector(".acp-markdown");
  if (!markdown) return [];
  // The trailing "Chat preview truncated" note follows every segment, so it
  // never shifts an index; diagram status and errors are excluded outright.
  return [...markdown.querySelectorAll(SEGMENT_SELECTOR)].filter(
    (element) => !element.closest(".acp-mermaid, [role=status], [role=alert]"),
  );
}

function segmentRange(segment: Element, start: number, end: number) {
  const walker = document.createTreeWalker(segment, NodeFilter.SHOW_TEXT);
  const range = document.createRange();
  let offset = 0;
  let started = false;
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    const length = node.textContent?.length ?? 0;
    if (!started && offset + length > start) {
      range.setStart(node, start - offset);
      started = true;
    }
    if (started && offset + length >= end) {
      range.setEnd(node, end - offset);
      return range;
    }
    offset += length;
  }
  return null;
}

/**
 * Ranges for the matches whose messages are mounted in this scrollport.
 * Each message and its segment list is looked up once, not once per match,
 * and matches in unmounted (virtualised) Turns cost one map miss.
 */
export function chatMatchRanges(
  scrollport: HTMLElement,
  matches: readonly ChatMatch[],
): Map<string, Range> {
  const ranges = new Map<string, Range>();
  if (!matches.length) return ranges;
  const messages = new Map<string, Element>();
  for (const element of scrollport.querySelectorAll<HTMLElement>(
    ".acp-message[data-item-id]",
  ))
    if (!messages.has(element.dataset.itemId!))
      messages.set(element.dataset.itemId!, element);
  const segments = new Map<string, Element[]>();
  for (const match of matches) {
    const message = messages.get(match.itemId);
    if (!message) continue;
    let list = segments.get(match.itemId);
    if (!list) segments.set(match.itemId, (list = segmentElements(message)));
    const segment = list[match.segment];
    const range = segment && segmentRange(segment, match.start, match.end);
    if (range) ranges.set(match.key, range);
  }
  return ranges;
}

/** Publishes one pane's ranges; null releases that pane's ranges only. */
export function publishChatHighlights(owner: symbol, ranges: Ranges | null) {
  if (ranges && !owners.has(owner) && owners.size >= MAX_HIGHLIGHT_OWNERS)
    return;
  if (ranges) owners.set(owner, ranges);
  else owners.delete(owner);
  if (
    typeof CSS === "undefined" ||
    !CSS.highlights ||
    typeof Highlight === "undefined"
  )
    return;
  for (const [name, field] of [
    ["sage-find", "other"],
    ["sage-find-current", "current"],
  ] as const) {
    const values = [...owners.values()].flatMap((value) => value[field]);
    if (values.length) {
      const highlight = new Highlight(...values);
      highlight.priority = field === "current" ? 1 : 0;
      CSS.highlights.set(name, highlight);
    } else CSS.highlights.delete(name);
  }
}
