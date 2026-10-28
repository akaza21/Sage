import {
  useEffect,
  useId,
  useRef,
  useState,
  type CSSProperties,
  type FocusEvent,
} from "react";
import { createPortal } from "react-dom";
import type { BrowserElementContext } from "../../domain/attachedContext";
import { MessageSquare, Trash2, X } from "../../ui/leanIcons";
import "./browser-element-chips.css";

const HIDE_MS = 150;
const CARD_WIDTH = 347;
const LABEL_CHARS = 48;

/** Contract §4: `Element N · <role> "<name>"`, cut to 48 characters. */
export function browserElementLabel({
  ordinal,
  role,
  name,
}: BrowserElementContext) {
  const label = `Element ${ordinal} · ${role}${name ? ` "${name}"` : ""}`;
  return label.length > LABEL_CHARS
    ? `${label.slice(0, LABEL_CHARS - 1)}…`
    : label;
}

/**
 * Annotate's elements in the composer (Codex ref 63): one chip per element
 * until there are several, then one "N annotations" chip. Hover or focus
 * opens a card listing each element's role and name, comment and page.
 */
export default function BrowserElementChips({
  elements,
  disabled,
  onRemove,
}: {
  elements: readonly BrowserElementContext[];
  disabled: boolean;
  onRemove: (element: BrowserElementContext) => void;
}) {
  const [open, setOpen] = useState(false);
  const [place, setPlace] = useState<CSSProperties>({});
  const chip = useRef<HTMLLIElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const card = useRef<HTMLDivElement>(null);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const id = useId();
  const show = () => {
    clearTimeout(timer.current);
    setOpen(true);
  };
  const hideSoon = () => {
    clearTimeout(timer.current);
    timer.current = setTimeout(() => setOpen(false), HIDE_MS);
  };
  const leave = ({ relatedTarget }: FocusEvent) => {
    const next = relatedTarget as Node | null;
    if (!chip.current?.contains(next) && !card.current?.contains(next))
      setOpen(false);
  };
  // Escape returns focus to the chip without opening the card again.
  const returning = useRef(false);
  const close = () => {
    setOpen(false);
    returning.current = true;
    trigger.current?.focus({ preventScroll: true });
    returning.current = false;
  };
  useEffect(() => () => clearTimeout(timer.current), []);
  useEffect(() => {
    if (!open) return;
    // The composer body scrolls, so the card hangs from the chip in the
    // viewport, 5 px above it and left-aligned with it.
    const placeCard = () => {
      const anchor = chip.current?.getBoundingClientRect();
      if (!anchor) return;
      const width = Math.min(CARD_WIDTH, innerWidth - 16);
      setPlace({
        width,
        left: Math.max(8, Math.min(anchor.left, innerWidth - width - 8)),
        bottom: innerHeight - anchor.top + 5,
      });
    };
    placeCard();
    addEventListener("resize", placeCard);
    addEventListener("scroll", placeCard, true);
    return () => {
      removeEventListener("resize", placeCard);
      removeEventListener("scroll", placeCard, true);
    };
  }, [open]);
  const single = elements.length === 1 ? elements[0] : null;
  const label = single
    ? browserElementLabel(single)
    : `${elements.length} annotations`;
  return (
    <li
      ref={chip}
      className="browser-element-chip"
      onMouseEnter={show}
      onMouseLeave={hideSoon}
      onBlur={leave}
      onKeyDown={(event) => {
        if (event.key !== "Escape" || !open) return;
        event.stopPropagation();
        close();
      }}
    >
      <button
        ref={trigger}
        type="button"
        className="browser-element-chip__trigger"
        aria-expanded={open}
        aria-controls={open ? id : undefined}
        title={single?.url}
        onFocus={() => returning.current || show()}
        onClick={() => {
          show();
          requestAnimationFrame(() =>
            card.current?.querySelector<HTMLElement>("button")?.focus(),
          );
        }}
      >
        <MessageSquare size={12} aria-hidden="true" />
        <span className="browser-element-chip__label">{label}</span>
      </button>
      <button
        type="button"
        className="browser-element-chip__remove"
        aria-label={single ? `Remove ${label}` : "Remove all annotations"}
        disabled={disabled}
        onClick={() => elements.forEach((element) => onRemove(element))}
      >
        <X size={10} strokeWidth={2.625} aria-hidden="true" />
      </button>
      {open
        ? createPortal(
            <div
              ref={card}
              id={id}
              role="dialog"
              aria-label="Page annotations"
              className="browser-element-card"
              style={place}
              onMouseEnter={show}
              onMouseLeave={hideSoon}
              onBlur={leave}
              onKeyDown={(event) => {
                if (event.key !== "Escape") return;
                event.stopPropagation();
                close();
              }}
            >
              {elements.map((element) => (
                <section
                  key={element.ordinal}
                  className="browser-element-card__entry"
                >
                  <header>
                    <code>{element.role}</code>
                    <span>{element.name || `Element ${element.ordinal}`}</span>
                    <button
                      type="button"
                      aria-label={`Delete element ${element.ordinal}`}
                      disabled={disabled}
                      onClick={() => onRemove(element)}
                    >
                      <Trash2 size={16} aria-hidden="true" />
                    </button>
                  </header>
                  <p>{element.comment || "No comment"}</p>
                  <p className="browser-element-card__page" title={element.url}>
                    {element.title || new URL(element.url).host}
                  </p>
                </section>
              ))}
            </div>,
            document.body,
          )
        : null}
    </li>
  );
}
