import { useEffect, useRef, useState } from "react";
import { attachedContextKey } from "../../domain/attachedContext";
import type { TranscriptQuoteContext } from "../../domain/transcriptQuote";
import { quoteAnswer, quoteRangeIn } from "./transcriptSelection";
import { annotationBadgeOrigin } from "./annotationGeometry";

type HighlightWindow = Window & {
  Highlight?: new (...ranges: Range[]) => unknown;
};
type HighlightCss = typeof CSS & { highlights?: Map<string, unknown> };

/** A quote's resolved answer and range, reused until that answer changes. */
type Resolved = {
  answer: HTMLElement | null;
  range: Range | null;
  dirty: boolean;
  /** Quoted from another Chat (a side chat's parent): never mounts here. */
  foreign?: boolean;
};

/** Cache ranges per quote; observe only its answer subtree. Scroll/resize only reposition; coalesce updates to one per frame. */
const notForeign = () => false;

export function TranscriptAnnotations({
  content,
  scrollport,
  quotes,
  editingKey,
  onOpen,
  isForeign = notForeign,
}: {
  content: HTMLElement;
  scrollport: HTMLElement;
  quotes: readonly TranscriptQuoteContext[];
  editingKey: string | null;
  onOpen: () => void;
  isForeign?: (quote: TranscriptQuoteContext) => boolean;
}) {
  const [badges, setBadges] = useState<
    {
      quote: TranscriptQuoteContext;
      number: number;
      left: number;
      top: number;
    }[]
  >([]);
  const [name] = useState(() => `sage-annotation-${crypto.randomUUID()}`);
  const cache = useRef(new Map<string, Resolved>());
  useEffect(() => {
    let frame = 0;
    // Resolve missing/replaced answers only on mount, resize, parent child-list changes or additions while waiting.
    let relookup = true;
    const resolved = cache.current;
    const observed = new Set<Node>();
    const schedule = () => {
      if (!frame) frame = requestAnimationFrame(update);
    };
    const mounted = () => {
      relookup = true;
      schedule();
    };
    const parents = new MutationObserver(mounted);
    const waiting = new MutationObserver(mounted);
    let waitingActive = false;
    const observer = new MutationObserver((records) => {
      for (const entry of resolved.values())
        if (records.some((record) => entry.answer?.contains(record.target)))
          entry.dirty = true;
      schedule();
    });
    const observe = () => {
      const entries = [...resolved.values()];
      const answers = new Set(
        entries.flatMap((entry) =>
          entry.answer?.isConnected ? [entry.answer] : [],
        ),
      );
      // Wait for disconnected answers to remount, never foreign answers.
      const wait = entries.some(
        (entry) => !entry.foreign && !entry.answer?.isConnected,
      );
      if (wait !== waitingActive) {
        if (wait) waiting.observe(content, { subtree: true, childList: true });
        else waiting.disconnect();
        waitingActive = wait;
      }
      if (
        answers.size === observed.size &&
        [...answers].every((answer) => observed.has(answer))
      )
        return;
      observer.disconnect();
      parents.disconnect();
      observed.clear();
      for (const answer of answers) {
        observer.observe(answer, {
          subtree: true,
          childList: true,
          characterData: true,
        });
        if (answer.parentNode)
          parents.observe(answer.parentNode, { childList: true });
        observed.add(answer);
      }
    };
    function update() {
      frame = 0;
      const keys = new Set(quotes.map(attachedContextKey));
      for (const key of resolved.keys())
        if (!keys.has(key)) resolved.delete(key);
      const bounds = scrollport.getBoundingClientRect();
      const ranges: Range[] = [];
      // Badge numbers follow draft order (Annotation N).
      const next = quotes.flatMap((quote, index) => {
        const key = attachedContextKey(quote);
        if (isForeign(quote)) {
          resolved.set(key, {
            answer: null,
            range: null,
            dirty: false,
            foreign: true,
          });
          return [];
        }
        let entry = resolved.get(key);
        const stale = !entry?.answer?.isConnected;
        if (!entry || entry.dirty || (stale && relookup)) {
          const answer =
            entry?.answer?.isConnected && !entry.dirty
              ? entry.answer
              : quoteAnswer(content, quote);
          entry = {
            answer,
            range: answer ? quoteRangeIn(answer, quote) : null,
            dirty: false,
          };
          resolved.set(key, entry);
        }
        const range = entry.range;
        if (!range || !entry.answer?.isConnected) return [];
        const rects = range.getClientRects();
        const last = rects[rects.length - 1];
        if (!last) return [];
        if (key === editingKey) ranges.push(range);
        return last.top >= bounds.top && last.top < bounds.bottom
          ? [{ quote, number: index + 1, ...annotationBadgeOrigin(last) }]
          : [];
      });
      relookup = false;
      observe();
      setBadges(next);
      const Highlight = (window as HighlightWindow).Highlight;
      if (Highlight && typeof CSS !== "undefined") {
        const registry = (CSS as HighlightCss).highlights;
        registry?.set(name, new Highlight(...ranges));
      }
    }
    const resize =
      typeof ResizeObserver === "undefined"
        ? null
        : new ResizeObserver(mounted);
    resize?.observe(content);
    scrollport.addEventListener("scroll", schedule);
    window.addEventListener("resize", schedule);
    update();
    return () => {
      cancelAnimationFrame(frame);
      observer.disconnect();
      parents.disconnect();
      waiting.disconnect();
      resize?.disconnect();
      scrollport.removeEventListener("scroll", schedule);
      window.removeEventListener("resize", schedule);
      if (typeof CSS !== "undefined")
        (CSS as HighlightCss).highlights?.delete(name);
    };
  }, [content, scrollport, quotes, editingKey, name, isForeign]);
  return (
    <>
      <style>{`::highlight(${name}) { background: var(--chat-annotation-highlight); color: var(--color-status-foreground); }`}</style>
      {badges.map(({ quote, number, left, top }) => (
        <button
          key={attachedContextKey(quote)}
          className="transcript-annotation-badge"
          style={{ left, top }}
          type="button"
          aria-label={`Annotation ${number}`}
          onClick={onOpen}
        >
          {number}
        </button>
      ))}
    </>
  );
}
