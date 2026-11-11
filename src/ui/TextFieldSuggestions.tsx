import { useLayoutEffect, type ReactNode, type RefObject } from "react";
import "../styles/text-field-suggestions.css";
import { MenuPopover } from "./MenuPopover";
import { ResourceStateNotice } from "./ResourceStateNotice";
import {
  TEXT_FIELD_SUGGESTIONS_MAX_RENDERED,
  suggestionOptionId,
  type TextFieldSuggestion,
  type TextFieldSuggestionsState,
} from "./textFieldSuggestionNavigation";

/**
 * Contiguous rows of one section. Headings show only when the rows span
 * more than one section.
 */
function sections(
  items: readonly TextFieldSuggestion[],
): { heading?: string; rows: readonly TextFieldSuggestion[] }[] {
  const out: { heading?: string; rows: TextFieldSuggestion[] }[] = [];
  for (const item of items) {
    const last = out.at(-1);
    if (last && last.heading === item.group) last.rows.push(item);
    else out.push({ heading: item.group, rows: [item] });
  }
  return out[1] ? out : [{ rows: items }];
}

/**
 * A bounded listbox that opens beside a text field (for example the chat
 * composer's `/` commands, later its `@` mentions). The field keeps the
 * focus and points at the active row with aria-activedescendant; rows take
 * pointer picks without taking the focus. The field's owner keeps a polite
 * status region mounted beside the field, so announcements are not lost
 * when the popover mounts.
 */
export function TextFieldSuggestions({
  anchorRef,
  ariaLabel,
  className,
  footer,
  items,
  activeId,
  listboxId,
  onActivate,
  onDismiss,
  onPick,
  state,
}: {
  anchorRef: RefObject<HTMLElement | null>;
  ariaLabel: string;
  className?: string;
  footer?: ReactNode;
  items: readonly TextFieldSuggestion[];
  activeId: string | null;
  listboxId: string;
  onActivate: (id: string) => void;
  onDismiss: () => void;
  onPick: (item: TextFieldSuggestion) => void;
  state: TextFieldSuggestionsState;
}) {
  const rendered = items.slice(0, TEXT_FIELD_SUGGESTIONS_MAX_RENDERED);
  const activeOptionId = activeId
    ? suggestionOptionId(listboxId, activeId)
    : null;
  useLayoutEffect(() => {
    if (!activeOptionId) return;
    anchorRef.current?.ownerDocument
      .getElementById(activeOptionId)
      ?.scrollIntoView?.({ block: "nearest" });
  }, [activeOptionId, anchorRef]);
  const option = (item: TextFieldSuggestion) => {
    const active = item.id === activeId;
    return (
      <div
        key={item.id}
        id={suggestionOptionId(listboxId, item.id)}
        role="option"
        aria-selected={active}
        aria-label={item.accessibleLabel}
        className="text-field-suggestions__option menu-item"
        data-active={active || undefined}
        onMouseMove={() => !active && onActivate(item.id)}
        onClick={() => onPick(item)}
      >
        <span className="text-field-suggestions__label">
          {item.icon}
          {item.label}
          {item.hint ? (
            <span className="text-field-suggestions__hint">{item.hint}</span>
          ) : null}
        </span>
        {item.description ? (
          <span className="text-field-suggestions__description">
            {item.description}
          </span>
        ) : null}
      </div>
    );
  };
  return (
    <MenuPopover
      ariaLabel={ariaLabel}
      className={["text-field-suggestions", className]
        .filter(Boolean)
        .join(" ")}
      focusStrategy="none"
      id={`${listboxId}-popover`}
      matchTriggerWidth
      placement="above-start"
      onDismiss={onDismiss}
      open
      role="none"
      triggerRef={anchorRef}
    >
      {state.kind === "ready" ? null : (
        <ResourceStateNotice
          state={state.kind === "empty" ? "idle" : state.kind}
          title={state.title}
          detail={"detail" in state ? state.detail : undefined}
        />
      )}
      <div
        id={listboxId}
        className="text-field-suggestions__list"
        role="listbox"
        aria-label={ariaLabel}
        // The field keeps the focus and the caret, also while the list's
        // scrollbar is dragged.
        onMouseDown={(event) => event.preventDefault()}
      >
        {sections(rendered).map(({ heading, rows }) =>
          heading ? (
            <div key={heading} role="group" aria-label={heading}>
              {/* The group's name, shown; the label reads it out. */}
              <div aria-hidden="true">{heading}</div>
              {rows.map(option)}
            </div>
          ) : (
            rows.map(option)
          ),
        )}
      </div>
      {items.length > rendered.length ? (
        <p className="text-field-suggestions__footer">
          {items.length - rendered.length} more — keep typing to narrow
        </p>
      ) : null}
      {footer ? (
        <p className="text-field-suggestions__footer">{footer}</p>
      ) : null}
    </MenuPopover>
  );
}
