import { useEffect } from "react";
import {
  MAX_TRANSCRIPT_QUOTE_BYTES,
  type TranscriptQuote,
  type TranscriptQuoteContext,
} from "../../domain/transcriptQuote";
import { utf8ByteLength } from "../../domain/text";

export const SELECTABLE_ANSWER = "[data-selectable-answer]";
const CHROME =
  'button, header, [role="toolbar"], [aria-hidden="true"], .acp-message__partial';
/** Fail closed beyond transcript retention limits. */
const MAX_ANSWER_TEXT_NODES = 20_000;
const MAX_ANSWER_CHARACTERS = 1_000_000;

type Box = { left: number; top: number; bottom: number };

export function selectionPopoverPosition(
  rect: Box,
  column: { left: number; top: number; width: number },
  size: { width: number; height: number },
) {
  const above = rect.top - column.top - size.height - 6;
  return {
    left: Math.max(
      8,
      Math.min(
        column.width - size.width - 8,
        rect.left - column.left - size.width / 2,
      ),
    ),
    top: above >= 0 ? above : rect.bottom - column.top + 6,
  };
}

function textNodes(answer: HTMLElement): Text[] {
  const walker = document.createTreeWalker(answer, NodeFilter.SHOW_TEXT, {
    acceptNode: (node) =>
      node.parentElement?.closest(CHROME) &&
      answer.contains(node.parentElement.closest(CHROME))
        ? NodeFilter.FILTER_REJECT
        : NodeFilter.FILTER_ACCEPT,
  });
  const nodes: Text[] = [];
  let total = 0;
  while (walker.nextNode()) {
    const node = walker.currentNode as Text;
    total += node.length;
    if (nodes.length >= MAX_ANSWER_TEXT_NODES || total > MAX_ANSWER_CHARACTERS)
      return [];
    nodes.push(node);
  }
  return nodes;
}

function offsetAt(
  answer: HTMLElement,
  nodes: Text[],
  container: Node,
  offset: number,
) {
  const prefix = document.createRange();
  prefix.selectNodeContents(answer);
  prefix.setEnd(container, offset);
  let result = 0;
  for (const node of nodes) {
    if (prefix.comparePoint(node, node.length) <= 0) result += node.length;
    else if (node === container) return result + offset;
    else break;
  }
  return result;
}

function answerOf(node: Node | null, content: HTMLElement) {
  const element =
    node?.nodeType === Node.ELEMENT_NODE
      ? (node as Element)
      : (node?.parentElement ?? null);
  const answer = element?.closest<HTMLElement>(SELECTABLE_ANSWER) ?? null;
  return answer && content.contains(answer) ? answer : null;
}

export function readTranscriptQuote(
  selection: Selection | null,
  content: HTMLElement,
) {
  if (!selection || selection.isCollapsed || selection.rangeCount !== 1)
    return null;
  const range = selection.getRangeAt(0);
  const answer = answerOf(range.startContainer, content);
  if (!answer || answerOf(range.endContainer, content) !== answer) return null;
  const start = range.startContainer;
  const startElement =
    start.nodeType === Node.ELEMENT_NODE
      ? (start as Element)
      : start.parentElement;
  if (
    startElement?.closest(CHROME) &&
    answer.contains(startElement.closest(CHROME))
  )
    return null;
  const nodes = textNodes(answer);
  const startOffset = offsetAt(answer, nodes, start, range.startOffset);
  const endOffset = offsetAt(
    answer,
    nodes,
    range.endContainer,
    range.endOffset,
  );
  const selected = nodes
    .map((node) => node.data)
    .join("")
    .slice(startOffset, endOffset);
  const text = selected.trim();
  if (
    !text ||
    utf8ByteLength(text) > MAX_TRANSCRIPT_QUOTE_BYTES ||
    text.includes("\0")
  )
    return null;
  const leading = selected.length - selected.trimStart().length;
  const quote: TranscriptQuote = {
    turnId: answer.dataset.selectableAnswer ?? "",
    itemId: answer.dataset.itemId ?? "",
    text,
    startOffset: startOffset + leading,
    endOffset: startOffset + leading + text.length,
  };
  if (!quote.turnId || !quote.itemId) return null;
  return { quote, range: range.cloneRange(), answer };
}

export function selectAnswerText(focused: HTMLElement, content: HTMLElement) {
  const answer = answerOf(focused, content);
  const nodes = answer && answer === focused ? textNodes(answer) : [];
  if (!nodes.length) return null;
  const range = document.createRange();
  range.setStart(nodes[0], 0);
  range.setEnd(nodes[nodes.length - 1], nodes[nodes.length - 1].length);
  const selection = window.getSelection();
  selection?.removeAllRanges();
  selection?.addRange(range);
  return readTranscriptQuote(selection, content);
}

export function quoteAnswer(
  content: HTMLElement,
  quote: TranscriptQuoteContext,
): HTMLElement | null {
  return (
    Array.from(content.querySelectorAll<HTMLElement>(SELECTABLE_ANSWER)).find(
      (candidate) =>
        candidate.dataset.selectableAnswer === quote.sourceTurnId &&
        candidate.dataset.itemId === quote.sourceItemId,
    ) ?? null
  );
}

export function quoteRange(
  content: HTMLElement,
  quote: TranscriptQuoteContext,
): Range | null {
  const answer = quoteAnswer(content, quote);
  return answer ? quoteRangeIn(answer, quote) : null;
}

export function quoteRangeIn(
  answer: HTMLElement,
  quote: TranscriptQuoteContext,
): Range | null {
  const nodes = textNodes(answer);
  const projected = nodes.map((node) => node.data).join("");
  if (projected.slice(quote.startOffset, quote.endOffset) !== quote.text)
    return null;
  const range = document.createRange();
  let offset = 0;
  let started = false;
  for (const node of nodes) {
    if (!started && quote.startOffset < offset + node.length) {
      range.setStart(node, quote.startOffset - offset);
      started = true;
    }
    if (started && quote.endOffset <= offset + node.length) {
      range.setEnd(node, quote.endOffset - offset);
      return range;
    }
    offset += node.length;
  }
  return null;
}

export function quoteContext(quote: TranscriptQuote): TranscriptQuoteContext {
  return {
    kind: "transcriptQuote",
    ordinal: 1,
    sourceTurnId: quote.turnId,
    sourceItemId: quote.itemId,
    text: quote.text,
    startOffset: quote.startOffset,
    endOffset: quote.endOffset,
  };
}

const bus = new EventTarget();

export type AnnotationRequest = "edit" | "show";

export function requestAnnotation(
  type: AnnotationRequest,
  chatId: string,
  key = "",
) {
  bus.dispatchEvent(new CustomEvent(`${type}:${chatId}`, { detail: key }));
}

export function useAnnotationRequest(
  type: AnnotationRequest,
  chatId: string | undefined,
  handler: ((key: string) => void) | undefined,
) {
  useEffect(() => {
    if (!chatId || !handler) return;
    const name = `${type}:${chatId}`;
    const listener = (event: Event) =>
      handler((event as CustomEvent<string>).detail);
    bus.addEventListener(name, listener);
    return () => bus.removeEventListener(name, listener);
  }, [type, chatId, handler]);
}
