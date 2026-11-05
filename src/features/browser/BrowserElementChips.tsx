import {
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type FocusEvent,
  type KeyboardEvent,
} from "react";
import { createPortal } from "react-dom";
import {
  attachedContextKey,
  isBrowserElement,
  type AttachedContext,
  type BrowserElementContext,
} from "../../domain/attachedContext";
import { Globe2, MessageSquare, Trash2, X } from "../../ui/leanIcons";
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
  const label = [...`Element ${ordinal} · ${role}${name ? ` "${name}"` : ""}`];
  return label.length > LABEL_CHARS
    ? `${label.slice(0, LABEL_CHARS - 1).join("")}…`
    : label.join("");
}

/** Contract §4: the tooltip names the page and the selector. */
const elementTip = ({ url, selector }: BrowserElementContext) =>
  `${url}\n${selector}`;

/**
 * Annotate's elements: one chip per element until there are
 * several, then one "N annotations" chip whose hover or focus opens a card
 * of each element's role, name, comment and page. With `onRemove` it is the
 * composer's draft row; without, the read-only row above a sent turn, which
 * also shows the turn's attached pages.
 */
export default function BrowserElementChips({
  contexts,
  disabled = false,
  onRemove,
}: {
  contexts: readonly AttachedContext[];
  disabled?: boolean;
  onRemove?: (context: AttachedContext) => void;
}) {
  const elements = contexts.filter(isBrowserElement);
  const chip = elements.length ? (
    <ElementChip elements={elements} disabled={disabled} onRemove={onRemove} />
  ) : null;
  if (onRemove) return chip;
  return (
    <ul className="browser-sent-context" aria-label="Page context">
      {chip}
      {contexts.map((context) =>
        context.kind === "browserPage" ? (
          <li
            key={attachedContextKey(context)}
            className="browser-element-chip"
          >
            <span className="browser-element-chip__trigger" title={context.url}>
              <Globe2 size={12} aria-hidden="true" />
              <span className="browser-element-chip__label">
                Page · {context.title || new URL(context.url).host}
              </span>
            </span>
          </li>
        ) : null,
      )}
    </ul>
  );
}

function ElementChip({
  elements,
  disabled,
  onRemove,
}: {
  elements: readonly BrowserElementContext[];
  disabled: boolean;
  onRemove?: (element: BrowserElementContext) => void;
}) {
  const [open, setOpen] = useState(false);
  const [place, setPlace] = useState<CSSProperties>({});
  const chip = useRef<HTMLLIElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const remove = useRef<HTMLButtonElement>(null);
  const card = useRef<HTMLDivElement>(null);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const refocus = useRef<number | null>(null);
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
  // Returning focus to the chip must not open the card again.
  const returning = useRef(false);
  const focusChip = (target = trigger.current) => {
    setOpen(false);
    returning.current = true;
    target?.focus({ preventScroll: true });
    returning.current = false;
  };
  // The chip leaves with its last element, so focus goes to the composer.
  const leaveForComposer = () =>
    chip.current?.closest("form")?.querySelector("textarea")?.focus();
  const deleteEntry = (element: BrowserElementContext, index: number) => {
    if (elements.length === 1) leaveForComposer();
    else refocus.current = Math.min(index, elements.length - 2);
    onRemove?.(element);
  };
  useLayoutEffect(() => {
    if (refocus.current === null) return;
    const buttons = card.current?.querySelectorAll("button");
    buttons?.item(refocus.current)?.focus();
    refocus.current = null;
  }, [elements]);
  // The card is portalled, so Tab past either end comes back to the chip.
  const tab = (event: KeyboardEvent) => {
    const buttons = [
      ...(card.current?.querySelectorAll<HTMLButtonElement>(
        "button:not(:disabled)",
      ) ?? []),
    ];
    const active = document.activeElement;
    if (event.shiftKey && (active === card.current || active === buttons[0])) {
      event.preventDefault();
      focusChip();
    } else if (
      !event.shiftKey &&
      (!buttons.length || active === buttons.at(-1))
    ) {
      event.preventDefault();
      focusChip(
        remove.current?.disabled === false ? remove.current : undefined,
      );
    }
  };
  useEffect(() => () => clearTimeout(timer.current), []);
  useEffect(() => {
    if (!open) return;
    // The card hangs 5 px above the chip in the viewport (the composer body
    // scrolls): from its left edge in the draft, its right edge when sent.
    const placeCard = () => {
      const anchor = chip.current?.getBoundingClientRect();
      if (!anchor) return;
      const width = Math.min(CARD_WIDTH, innerWidth - 16);
      const start = onRemove ? anchor.left : anchor.right - width;
      setPlace({
        width,
        left: Math.max(8, Math.min(start, innerWidth - width - 8)),
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
  }, [open, onRemove]);
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
        focusChip();
      }}
    >
      <button
        ref={trigger}
        type="button"
        className="browser-element-chip__trigger"
        aria-label={single ? undefined : `${label} on the page`}
        aria-expanded={open}
        aria-controls={open ? id : undefined}
        title={single ? elementTip(single) : undefined}
        onFocus={() => returning.current || show()}
        onClick={() => {
          show();
          requestAnimationFrame(() => card.current?.focus());
        }}
      >
        <MessageSquare size={12} aria-hidden="true" />
        <span className="browser-element-chip__label">{label}</span>
      </button>
      {onRemove ? (
        <button
          ref={remove}
          type="button"
          className="browser-element-chip__remove"
          aria-label={
            single ? `Remove ${label}` : "Remove all page annotations"
          }
          disabled={disabled}
          onClick={() => {
            leaveForComposer();
            elements.forEach((element) => onRemove(element));
          }}
        >
          <X size={10} strokeWidth={2.625} aria-hidden="true" />
        </button>
      ) : null}
      {open
        ? createPortal(
            <div
              ref={card}
              id={id}
              role="dialog"
              tabIndex={-1}
              aria-label="Page annotations"
              className="browser-element-card"
              style={place}
              onMouseEnter={show}
              onMouseLeave={hideSoon}
              onBlur={leave}
              onKeyDown={(event) => {
                if (event.key === "Tab") tab(event);
                if (event.key !== "Escape") return;
                event.stopPropagation();
                focusChip();
              }}
            >
              {elements.map((element, index) => (
                <section
                  key={attachedContextKey(element)}
                  className="browser-element-card__entry"
                >
                  <header title={elementTip(element)}>
                    <code>{element.role}</code>
                    <span>{element.name || `Element ${element.ordinal}`}</span>
                    {onRemove ? (
                      <button
                        type="button"
                        aria-label={`Delete element ${element.ordinal}`}
                        disabled={disabled}
                        onClick={() => deleteEntry(element, index)}
                      >
                        <Trash2 size={16} aria-hidden="true" />
                      </button>
                    ) : null}
                  </header>
                  <p>{element.comment || "No comment"}</p>
                  <p
                    className="browser-element-card__page"
                    title={elementTip(element)}
                  >
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
