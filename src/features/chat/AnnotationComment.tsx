import { useEffect, useLayoutEffect, useRef, useState } from "react";
import {
  MAX_TRANSCRIPT_QUOTE_BYTES,
  type TranscriptQuoteContext,
} from "../../domain/transcriptQuote";
import { utf8ByteLength } from "../../domain/text";
import { quoteRange, SELECTABLE_ANSWER } from "./transcriptSelection";
import { annotationFieldBox } from "./annotationGeometry";
import { Check } from "../../ui/leanIcons";

const FIELD_HEIGHT = 44;

export function AnnotationComment({
  quote,
  content,
  scrollport,
  onSave,
  active = true,
}: {
  quote: TranscriptQuoteContext;
  content: HTMLElement;
  scrollport: HTMLElement;
  onSave: (comment: string, restoreFocus: boolean) => void;
  active?: boolean;
}) {
  const [comment, setComment] = useState(quote.comment ?? "");
  const [position, setPosition] = useState({ left: 0, top: 0, width: 270 });
  const [error, setError] = useState("");
  const input = useRef<HTMLInputElement>(null);
  const latest = useRef({ comment, onSave });
  useLayoutEffect(() => {
    latest.current = { comment, onSave };
  });
  useEffect(() => {
    if (!active) latest.current.onSave(latest.current.comment, false);
  }, [active]);
  useLayoutEffect(() => {
    const update = () => {
      const range = quoteRange(content, quote);
      const rects = range?.getClientRects();
      const viewport = scrollport.getBoundingClientRect();
      const column = (
        range?.startContainer.parentElement?.closest(SELECTABLE_ANSWER) ??
        content
      ).getBoundingClientRect();
      if (!rects?.length) {
        setPosition({
          left: column.left,
          top: viewport.bottom - FIELD_HEIGHT - 8,
          width: Math.min(270, column.width),
        });
        return;
      }
      setPosition(
        annotationFieldBox(
          { first: rects[0], last: rects[rects.length - 1] },
          column.width,
          viewport,
          FIELD_HEIGHT,
        ),
      );
    };
    update();
    input.current?.focus({ preventScroll: true });
    scrollport.addEventListener("scroll", update);
    window.addEventListener("resize", update);
    return () => {
      scrollport.removeEventListener("scroll", update);
      window.removeEventListener("resize", update);
    };
  }, [content, scrollport, quote]);
  return (
    <form
      className="annotation-comment"
      aria-label="Annotation comment"
      style={position}
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node))
          onSave(comment, false);
      }}
      onSubmit={(event) => {
        event.preventDefault();
        onSave(comment, true);
      }}
      onKeyDown={(event) => {
        if (event.key !== "Escape") return;
        event.preventDefault();
        event.stopPropagation();
        onSave(quote.comment ?? "", true);
      }}
    >
      <input
        ref={input}
        value={comment}
        aria-label="Annotation comment"
        placeholder={"Add an optional comment…"}
        onChange={(event) => {
          if (utf8ByteLength(event.target.value) > MAX_TRANSCRIPT_QUOTE_BYTES)
            return setError("Comments are limited to 4096 bytes");
          setComment(event.target.value);
          setError("");
        }}
      />
      <button type="submit" aria-label="Save annotation">
        <Check size={12} strokeWidth={2.625} aria-hidden="true" />
      </button>
      {error ? (
        <span className="annotation-comment__error" role="alert">
          {error}
        </span>
      ) : null}
    </form>
  );
}
