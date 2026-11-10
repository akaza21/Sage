import {
  useEffect,
  useCallback,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
  type RefObject,
} from "react";
import { flushSync } from "react-dom";
import { isImeComposing } from "./imeComposition";
import { listIndexForKey } from "./listNavigation";
import { PickerOption } from "./PickerOption";
import { MenuPopover, type MenuDismissReason } from "./MenuPopover";
import type { MenuPlacement } from "./menuPopoverPosition";
import { ResourceStateNotice } from "./ResourceStateNotice";
import type { SearchablePickerOption } from "./SearchablePicker";
import {
  SEARCHABLE_PICKER_DEBOUNCE_MS,
  SEARCHABLE_PICKER_MAX_RENDERED,
  SEARCHABLE_PICKER_PAGE_SIZE,
  searchablePickerOptionId as optionId,
} from "./searchablePickerLoad";

const EMPTY_CANDIDATES: readonly SearchablePickerOption[] = [];

/**
 * The open list of a SearchablePicker: its search field, rows and keyboard
 * handling. It loads on the first open (preloaded before) and then stays
 * mounted. The picker keeps the typed query (`draft`), including what was
 * typed on the trigger before this list arrived.
 */
export function SearchablePickerPopover({
  ariaLabel,
  candidates,
  close,
  disabled,
  draft,
  expanded,
  focusMoved,
  footer,
  id,
  onOptionRender,
  onSelect,
  open,
  popoverClassName,
  popoverGap,
  popoverPlacement,
  portalTarget,
  searchable,
  sectionLabel,
  selected,
  selectedId,
  setDraft,
  setOpen,
  state,
  stateDetail,
  stateTitle,
  triggerRef,
}: {
  ariaLabel: string;
  candidates: readonly SearchablePickerOption[];
  close: (reason?: MenuDismissReason) => void;
  disabled: boolean;
  draft: string;
  expanded: boolean;
  /** True when focus moved on while this list was loading for an open. */
  focusMoved: () => boolean;
  footer?: string;
  id: string;
  onOptionRender?: (id: string) => void;
  onSelect: (id: string) => void;
  open: boolean;
  popoverClassName?: string;
  popoverGap?: number;
  popoverPlacement?: MenuPlacement;
  portalTarget: "closest-dialog" | "closest-popover" | "document";
  searchable: boolean;
  sectionLabel?: string;
  selected: SearchablePickerOption | undefined;
  selectedId: string | null;
  setDraft: (draft: string) => void;
  setOpen: (open: boolean) => void;
  state?: "error" | "idle" | "loading";
  stateDetail?: string;
  stateTitle?: string;
  triggerRef: RefObject<HTMLButtonElement | null>;
}) {
  const inputRef = useRef<HTMLInputElement>(null);
  const [query, setQuery] = useState(draft);
  // A list that arrives after focus moved on leaves it there for this open.
  const [holdFocus, setHoldFocus] = useState(focusMoved);
  if (!open && holdFocus) setHoldFocus(false);
  const [activeId, setActiveId] = useState<string | null>(null);
  // Every close clears the active row, whichever side closed the list.
  if (!open && activeId !== null) setActiveId(null);
  const visible = useMemo(() => {
    if (!expanded) return EMPTY_CANDIDATES;
    const normalized = query.trim().toLocaleLowerCase();
    return candidates.filter((candidate) =>
      candidate.label.toLocaleLowerCase().includes(normalized),
    );
  }, [candidates, query, expanded]);
  const rendered = useMemo(
    () => visible.slice(0, SEARCHABLE_PICKER_MAX_RENDERED),
    [visible],
  );
  const selectable = useMemo(
    () => rendered.filter((candidate) => !candidate.disabledReason),
    [rendered],
  );
  const active =
    selectable.find((candidate) => candidate.id === activeId) ?? null;

  useEffect(() => {
    if (!open || disabled) return;
    const timer = window.setTimeout(
      () => setQuery(draft),
      SEARCHABLE_PICKER_DEBOUNCE_MS,
    );
    return () => window.clearTimeout(timer);
  }, [disabled, draft, open]);

  useLayoutEffect(() => {
    if (!open || disabled || !searchable || holdFocus) return;
    inputRef.current?.focus();
  }, [disabled, holdFocus, open, searchable]);
  useLayoutEffect(() => {
    if (!active) return;
    triggerRef.current?.ownerDocument
      .getElementById(optionId(id, active.id))
      ?.scrollIntoView?.({ block: "nearest" });
  }, [active, id, triggerRef]);

  const dismiss = useCallback(
    (reason?: MenuDismissReason) => {
      setActiveId(null);
      close(reason);
    },
    [close],
  );
  // `selectable` holds only the candidates that can be picked.
  const moveActive = (key: string) => {
    const next = listIndexForKey(
      key,
      selectable.findIndex((candidate) => candidate.id === activeId),
      selectable.map(() => false),
      {
        orientation: "vertical",
        wrap: true,
        pageSize: SEARCHABLE_PICKER_PAGE_SIZE,
      },
    );
    if (next !== null) setActiveId(selectable[next].id);
  };
  const selectCandidate = useCallback(
    (next: string) => {
      if (disabled) return;
      onSelect(next);
      dismiss();
      triggerRef.current?.focus();
    },
    [dismiss, disabled, onSelect, triggerRef],
  );
  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    // Keys that confirm an input method composition belong to the field.
    if (disabled || isImeComposing(event)) return;
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      if (!open) setOpen(true);
      moveActive(event.key);
    } else if (event.key === "Home" || event.key === "End") {
      if (selectable.length === 0) return;
      event.preventDefault();
      moveActive(event.key);
    } else if (event.key === "PageDown" || event.key === "PageUp") {
      event.preventDefault();
      moveActive(event.key);
    } else if (event.key === "Enter") {
      event.preventDefault();
      if (active) selectCandidate(active.id);
    } else if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      dismiss("escape");
    } else if (event.key === "Tab") {
      if (portalTarget !== "document") {
        flushSync(() => dismiss("tab"));
        triggerRef.current?.focus();
        return;
      }
      triggerRef.current?.focus();
      dismiss("tab");
    }
  };

  const listed = expanded ? rendered : EMPTY_CANDIDATES;

  return (
    <MenuPopover
      ariaLabel={ariaLabel}
      className={[
        "searchable-picker__popover",
        "menu-popover--editor",
        popoverClassName,
      ]
        .filter(Boolean)
        .join(" ")}
      focusStrategy={searchable ? "none" : "roving"}
      holdFocus={holdFocus}
      focusKey={
        searchable
          ? undefined
          : selectable.map((candidate) => candidate.id).join("\0")
      }
      initialFocus="selected"
      id={`${id}-popover`}
      matchTriggerWidth
      gap={popoverGap}
      placement={popoverPlacement}
      onDismiss={dismiss}
      open={expanded}
      portalTarget={portalTarget}
      role="none"
      triggerRef={triggerRef}
    >
      {searchable ? (
        <input
          ref={inputRef}
          className="searchable-picker__input"
          role="combobox"
          aria-label={ariaLabel}
          aria-activedescendant={active ? optionId(id, active.id) : undefined}
          aria-autocomplete="list"
          aria-controls={`${id}-listbox`}
          aria-expanded={expanded}
          value={draft}
          onChange={(event) => {
            setDraft(event.target.value);
            setActiveId(null);
          }}
          onKeyDown={onKeyDown}
        />
      ) : null}
      {selected ? (
        <p className="searchable-picker__current">Current: {selected.label}</p>
      ) : null}
      {sectionLabel ? (
        <p className="searchable-picker__section-label">{sectionLabel}</p>
      ) : null}
      {state ? (
        <ResourceStateNotice
          detail={stateDetail}
          state={state}
          title={stateTitle ?? ariaLabel}
        />
      ) : null}
      {!state && visible.length === 0 ? (
        <ResourceStateNotice
          state="idle"
          title={`No ${ariaLabel.toLocaleLowerCase()} found.`}
        />
      ) : null}
      <div id={`${id}-listbox`} role="listbox" aria-label={ariaLabel}>
        {listed.map((candidate) => (
          <PickerOption
            key={candidate.id}
            active={candidate.id === active?.id}
            candidate={candidate}
            id={optionId(id, candidate.id)}
            onActivate={searchable ? setActiveId : undefined}
            onRender={onOptionRender}
            onSelect={selectCandidate}
            selected={candidate.id === selectedId}
            disabled={disabled}
          />
        ))}
        {visible.length > rendered.length ? (
          <p className="searchable-picker__limit">
            {visible.length - rendered.length} more — refine
          </p>
        ) : null}
      </div>
      {footer ? <p className="searchable-picker__footer">{footer}</p> : null}
    </MenuPopover>
  );
}
