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
import { useActiveOption } from "./listNavigation";
import { PickerOption } from "./PickerOption";
import { Search } from "./leanIcons";
import { MenuPopover, type MenuDismissReason } from "./MenuPopover";
import type { MenuPlacement } from "./menuPopoverPosition";
import { ResourceStateNotice } from "./ResourceStateNotice";
import type { SearchablePickerOption } from "./SearchablePicker";
import {
  SEARCHABLE_PICKER_DEBOUNCE_MS,
  SEARCHABLE_PICKER_MAX_RENDERED,
  searchablePickerOptionId as optionId,
} from "./searchablePickerLoad";

export function SearchablePickerPopover({
  ariaLabel,
  candidates,
  close,
  draft,
  query,
  onQueryChange,
  focusMoved,
  footer,
  id,
  onOptionRender,
  onSelect,
  popoverClassName,
  popoverGap,
  popoverPlacement,
  portalTarget,
  searchable,
  searchPlaceholder = "Search…",
  menuAppearance = false,
  keyboardOpen,
  sectionLabel,
  selected,
  selectedId,
  setDraft,
  state,
  stateDetail,
  stateTitle,
  triggerRef,
}: {
  ariaLabel: string;
  candidates: readonly SearchablePickerOption[];
  close: (reason?: MenuDismissReason) => void;
  draft: string;
  query: string;
  onQueryChange: (query: string) => void;
  focusMoved: () => boolean;
  footer?: string;
  id: string;
  onOptionRender?: (id: string) => void;
  onSelect: (id: string) => void;
  popoverClassName?: string;
  popoverGap?: number;
  popoverPlacement?: MenuPlacement;
  portalTarget: "closest-dialog" | "closest-popover" | "document";
  searchable: boolean;
  searchPlaceholder?: string;
  menuAppearance?: boolean;
  keyboardOpen: boolean;
  sectionLabel?: string;
  selected: SearchablePickerOption | undefined;
  selectedId: string | null;
  setDraft: (draft: string) => void;
  state?: "error" | "idle" | "loading";
  stateDetail?: string;
  stateTitle?: string;
  triggerRef: RefObject<HTMLButtonElement | null>;
}) {
  const inputRef = useRef<HTMLInputElement>(null);
  const [keyboard, setKeyboard] = useState(keyboardOpen);
  useLayoutEffect(() => {
    onQueryChange(query);
  }, [query, onQueryChange]);
  const [holdFocus] = useState(focusMoved);
  const visible = useMemo(() => {
    const normalized = query.trim().toLocaleLowerCase();
    return candidates.filter(
      (candidate) =>
        candidate.alwaysVisible ||
        candidate.label.toLocaleLowerCase().includes(normalized),
    );
  }, [candidates, query]);
  const rendered = useMemo(
    () => visible.slice(0, SEARCHABLE_PICKER_MAX_RENDERED),
    [visible],
  );
  const selectableIds = useMemo(
    () =>
      rendered
        .filter((candidate) => !candidate.disabledReason)
        .map((candidate) => candidate.id),
    [rendered],
  );
  const option = useActiveOption(selectableIds, "none");
  const setActiveId = option.setActiveId;
  const activateFromPointer = useCallback(
    (id: string) => {
      setActiveId(id);
      setKeyboard(false);
    },
    [setActiveId],
  );
  const active = option.activeId;

  useEffect(() => {
    const timer = window.setTimeout(() => {
      onQueryChange(draft);
    }, SEARCHABLE_PICKER_DEBOUNCE_MS);
    return () => window.clearTimeout(timer);
  }, [draft, onQueryChange]);

  useLayoutEffect(() => {
    if (!searchable || holdFocus) return;
    inputRef.current?.focus();
  }, [holdFocus, searchable]);
  useLayoutEffect(() => {
    if (active === null) return;
    triggerRef.current?.ownerDocument
      .getElementById(optionId(id, active))
      ?.scrollIntoView?.({ block: "nearest" });
  }, [active, id, triggerRef]);

  const moveActive = option.move;
  const selectCandidate = useCallback(
    (next: string) => {
      onSelect(next);
      setDraft("");
      onQueryChange("");
      setActiveId(null);
      close();
      triggerRef.current?.focus();
    },
    [close, onSelect, onQueryChange, setActiveId, setDraft, triggerRef],
  );
  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    const { key } = event;
    setKeyboard(true);
    if (isImeComposing(event)) return;
    if (
      ["ArrowDown", "ArrowUp", "Home", "End", "PageDown", "PageUp"].includes(
        key,
      )
    ) {
      if ((key === "Home" || key === "End") && selectableIds.length === 0)
        return;
      event.preventDefault();
      moveActive(key);
    } else if (key === "Enter") {
      event.preventDefault();
      if (active !== null) selectCandidate(active);
    } else if (key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      close("escape");
    } else if (key === "Tab") {
      if (portalTarget !== "document") {
        flushSync(() => close("tab"));
        triggerRef.current?.focus();
        return;
      }
      triggerRef.current?.focus();
      close("tab");
    }
  };

  return (
    <MenuPopover
      ariaLabel={ariaLabel}
      className={[
        "searchable-picker__popover menu-popover--editor",
        menuAppearance ? "searchable-picker__popover--menu" : null,
        keyboard ? "searchable-picker__popover--keyboard" : null,
        popoverClassName,
      ]
        .filter(Boolean)
        .join(" ")}
      focusStrategy={searchable ? "none" : "roving"}
      holdFocus={holdFocus}
      focusKey={searchable ? undefined : selectableIds.join("\0")}
      initialFocus="selected"
      id={`${id}-popover`}
      matchTriggerWidth={!menuAppearance}
      gap={popoverGap}
      placement={popoverPlacement}
      onDismiss={close}
      open
      portalTarget={portalTarget}
      role="none"
      triggerRef={triggerRef}
    >
      {searchable ? (
        <div
          className={
            menuAppearance ? "searchable-picker__search-row" : undefined
          }
        >
          {menuAppearance ? <Search size={14} aria-hidden="true" /> : null}
          <input
            ref={inputRef}
            className="searchable-picker__input"
            role="combobox"
            aria-label={ariaLabel}
            aria-activedescendant={
              active !== null ? optionId(id, active) : undefined
            }
            aria-autocomplete="list"
            aria-controls={`${id}-listbox`}
            aria-expanded
            placeholder={searchPlaceholder}
            value={draft}
            onChange={(event) => {
              setDraft(event.target.value);
              setActiveId(null);
            }}
            onKeyDown={onKeyDown}
            onPointerDown={() => setKeyboard(false)}
          />
        </div>
      ) : null}
      {selected && !menuAppearance ? (
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
        {rendered.map((candidate) => (
          <PickerOption
            key={candidate.id}
            active={candidate.id === active}
            candidate={candidate}
            id={optionId(id, candidate.id)}
            onActivate={searchable ? activateFromPointer : undefined}
            onRender={onOptionRender}
            onSelect={selectCandidate}
            selected={candidate.id === selectedId}
            showSelectedCheck={menuAppearance}
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
