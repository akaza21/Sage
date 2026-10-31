import {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { createPortal } from "react-dom";
import {
  attachedContextKey,
  MAX_ATTACHED_CONTEXTS_PER_TURN,
  type AttachedContext,
} from "../../domain/attachedContext";
import {
  isTranscriptQuote,
  type TranscriptQuoteContext,
} from "../../domain/transcriptQuote";
import { leanIcon } from "../../ui/leanIconCore";
import { X } from "../../ui/leanIcons";
import { isSideChatId } from "../../workbench/sideChatState";
import { SentBrowserContext } from "../browser/BrowserElementChipsSlot";
import "./transcript-interactions.css";
import { requestAnnotation, useAnnotationRequest } from "./transcriptSelection";

const CommentSquare = leanIcon("annotation-comment-square", [
  [
    "path",
    {
      d: "M7 4h10a3 3 0 0 1 3 3v8a3 3 0 0 1-3 3h-3l-2 3-2-3H7a3 3 0 0 1-3-3V7a3 3 0 0 1 3-3Z",
    },
  ],
  ["path", { d: "M8 9h8M8 13h5" }],
]);
const AnnotationPencil = leanIcon("annotation-pencil", [
  ["path", { d: "m16 3 5 5M3 21l5-1L21 7a2 2 0 0 0-5-5L3 15Z" }],
]);
const AnnotationTrash = leanIcon("annotation-trash", [
  ["path", { d: "M3 6h18M9 6V3h6v3M5 6l1 15h12l1-15M10 10v7M14 10v7" }],
]);

const HIDE_DELAY_MS = 150;
const CARD_WIDTH = 342;

/**
 * "{n} annotation(s)" chip with its hover card. A draft chip (onDelete given)
 * offers remove-all, edit and delete; a sent chip is read-only.
 */
export function AnnotationChip({
  quotes,
  chatId,
  selection = false,
  disabled = false,
  onEdit,
  onDelete,
  onClear,
}: {
  quotes: readonly TranscriptQuoteContext[];
  chatId?: string;
  selection?: boolean;
  disabled?: boolean;
  onEdit?: (quote: TranscriptQuoteContext) => void;
  onDelete?: (quote: TranscriptQuoteContext) => void;
  onClear?: () => void;
}) {
  const draft = Boolean(onDelete);
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState({ left: 0, top: 0, width: 0 });
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const card = useRef<HTMLDivElement>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const id = useId();
  const cancelHide = () => {
    if (timer.current) clearTimeout(timer.current);
    timer.current = null;
  };
  const hideSoon = () => {
    cancelHide();
    timer.current = setTimeout(() => setOpen(false), HIDE_DELAY_MS);
  };
  /** Escape returns focus to the chip without reopening the card. */
  const returning = useRef(false);
  const closeToTrigger = () => {
    returning.current = true;
    setOpen(false);
    trigger.current?.focus();
    returning.current = false;
  };
  useEffect(() => cancelHide, []);
  useAnnotationRequest(
    "show",
    draft ? chatId : undefined,
    useCallback(() => setOpen(true), []),
  );
  useEffect(() => {
    if (!open) return;
    const outside = (event: PointerEvent) => {
      const target = event.target as Node;
      if (!root.current?.contains(target) && !card.current?.contains(target))
        setOpen(false);
    };
    document.addEventListener("pointerdown", outside);
    return () => document.removeEventListener("pointerdown", outside);
  }, [open]);
  useLayoutEffect(() => {
    if (!open) return;
    const place = () => {
      const anchor = trigger.current?.getBoundingClientRect();
      const panel = card.current?.getBoundingClientRect();
      if (!anchor || !panel) return;
      const column = root.current
        ?.closest(".composer, .acp-turn__items")
        ?.getBoundingClientRect();
      const width = Math.min(
        CARD_WIDTH,
        column?.width || CARD_WIDTH,
        window.innerWidth - 32,
      );
      setPosition({
        width,
        left: Math.max(
          8,
          Math.min(
            window.innerWidth - width - 8,
            draft ? anchor.left : anchor.right - width,
          ),
        ),
        top: Math.max(8, anchor.top - panel.height - 6),
      });
    };
    place();
    window.addEventListener("resize", place);
    window.addEventListener("scroll", place, true);
    const observer =
      typeof ResizeObserver === "undefined" ? null : new ResizeObserver(place);
    if (card.current) observer?.observe(card.current);
    return () => {
      observer?.disconnect();
      window.removeEventListener("resize", place);
      window.removeEventListener("scroll", place, true);
    };
  }, [open, quotes, draft]);
  if (!quotes.length) return null;
  const noun = selection ? "selection" : "annotation";
  const label = `${quotes.length} ${noun}${quotes.length === 1 ? "" : "s"}`;
  return (
    <div
      ref={root}
      className={`annotation-chip annotation-chip--${draft ? "draft" : "sent"}`}
      onMouseEnter={() => {
        cancelHide();
        setOpen(true);
      }}
      onMouseLeave={hideSoon}
      onBlur={(event) => {
        const next = event.relatedTarget as Node | null;
        if (
          !event.currentTarget.contains(next) &&
          !card.current?.contains(next)
        )
          setOpen(false);
      }}
      onKeyDown={(event) => {
        if (event.key !== "Escape" || !open) return;
        event.stopPropagation();
        closeToTrigger();
      }}
    >
      <button
        ref={trigger}
        type="button"
        className="button--compact annotation-chip__trigger"
        aria-expanded={open}
        aria-controls={open ? id : undefined}
        onFocus={() => {
          if (!returning.current) setOpen(true);
        }}
        onClick={() => {
          setOpen(true);
          requestAnimationFrame(() =>
            (
              card.current?.querySelector<HTMLElement>("button") ?? card.current
            )?.focus(),
          );
        }}
      >
        <CommentSquare size={12} aria-hidden="true" />
        <span className="annotation-chip__label">{label}</span>
      </button>
      {onClear ? (
        <button
          type="button"
          className="annotation-chip__remove"
          aria-label={`Remove all ${noun}s`}
          disabled={disabled}
          onClick={onClear}
        >
          <X size={10} strokeWidth={2.625} aria-hidden="true" />
        </button>
      ) : null}
      {open
        ? createPortal(
            <div
              ref={card}
              id={id}
              tabIndex={-1}
              style={position.width ? position : { visibility: "hidden" }}
              className="annotation-card"
              role="dialog"
              aria-label={selection ? "Selections" : "Annotations"}
              onMouseEnter={cancelHide}
              onMouseLeave={hideSoon}
              onKeyDown={(event) => {
                if (event.key !== "Escape") return;
                event.stopPropagation();
                closeToTrigger();
              }}
            >
              {quotes
                .slice(0, MAX_ATTACHED_CONTEXTS_PER_TURN)
                .map((quote, index) => (
                  <section
                    key={attachedContextKey(quote)}
                    className="annotation-card__entry"
                  >
                    <span className="annotation-card__number">
                      {index + 1}.
                    </span>
                    {draft ? (
                      <div className="annotation-card__actions">
                        {onEdit ? (
                          <button
                            type="button"
                            aria-label={`Edit annotation ${index + 1}`}
                            disabled={disabled}
                            onClick={() => {
                              setOpen(false);
                              onEdit(quote);
                            }}
                          >
                            <AnnotationPencil size={16} aria-hidden="true" />
                          </button>
                        ) : null}
                        <button
                          type="button"
                          aria-label={`Delete ${noun} ${index + 1}`}
                          disabled={disabled}
                          onClick={() => onDelete?.(quote)}
                        >
                          <AnnotationTrash size={16} aria-hidden="true" />
                        </button>
                      </div>
                    ) : null}
                    <dl>
                      <dt>Selected text:</dt>
                      <dd>{quote.text}</dd>
                      {quote.comment ? (
                        <>
                          <dt>User comment:</dt>
                          <dd>{quote.comment}</dd>
                        </>
                      ) : null}
                    </dl>
                  </section>
                ))}
            </div>,
            document.body,
          )
        : null}
    </div>
  );
}

/** Read-only chip above a sent user bubble; the turn's contexts are filtered here. */
export function SentAnnotations({
  contexts,
  chatId,
}: {
  contexts: readonly AttachedContext[];
  chatId?: string;
}) {
  return (
    <>
      <AnnotationChip
        // A side chat's sent quotes are selections, as in its composer
        //.
        selection={isSideChatId(chatId)}
        quotes={contexts.filter(isTranscriptQuote)}
      />
      <SentBrowserContext contexts={contexts} />
    </>
  );
}

/** The aggregated "{n} annotation(s)" / "{n} selection(s)" row above the textarea. */
export function ComposerAnnotations({
  contexts,
  disabled,
  onRemove,
  chatId,
}: {
  contexts: readonly AttachedContext[];
  disabled: boolean;
  onRemove: (context: AttachedContext) => void;
  chatId: string;
}) {
  const side = isSideChatId(chatId);
  const quotes = contexts.filter(isTranscriptQuote);
  return (
    <div className="composer__annotation-row">
      <AnnotationChip
        chatId={chatId}
        quotes={quotes}
        selection={side}
        disabled={disabled}
        onClear={() => quotes.forEach(onRemove)}
        onDelete={onRemove}
        onEdit={
          side
            ? undefined
            : (quote) =>
                requestAnnotation("edit", chatId, attachedContextKey(quote))
        }
      />
    </div>
  );
}
