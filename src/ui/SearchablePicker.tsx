import {
  useEffect,
  useCallback,
  useId,
  useLayoutEffect,
  memo,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
} from "react";
import { flushSync } from "react-dom";
import { Check, ChevronDown } from "./leanIcons";

import { MenuPopover, type MenuDismissReason } from "./MenuPopover";
import { ResourceStateNotice } from "./ResourceStateNotice";

export const SEARCHABLE_PICKER_MAX_CANDIDATES = 500;
export const SEARCHABLE_PICKER_MAX_RENDERED = 200;
export const SEARCHABLE_PICKER_DEBOUNCE_MS = 50;
export const SEARCHABLE_PICKER_PAGE_SIZE = 10;

export interface SearchablePickerOption {
  id: string;
  label: string;
  accessibleLabel?: string;
  triggerLabel?: string;
  disabledReason?: string;
  icon?: "claude" | "codex" | "opencode";
  logo?: string;
  secondaryLabel?: string;
}

const EMPTY_CANDIDATES: readonly SearchablePickerOption[] = [];

const PickerOption = memo(function PickerOption({
  active,
  candidate,
  id,
  onSelect,
  onActivate,
  onOptionElement,
  onRender,
  selected,
}: {
  active: boolean;
  candidate: SearchablePickerOption;
  id: string;
  onActivate: (id: string) => void;
  onOptionElement: (id: string, element: HTMLButtonElement | null) => void;
  onRender?: (id: string) => void;
  onSelect: (id: string) => void;
  selected: boolean;
}) {
  onRender?.(candidate.id);
  const disabled = Boolean(candidate.disabledReason);
  const logo =
    candidate.logo ??
    (candidate.icon ? `/harness-${candidate.icon}.svg` : null);
  return (
    <button
      ref={(element) => onOptionElement(candidate.id, element)}
      id={id}
      type="button"
      role="option"
      aria-label={candidate.accessibleLabel}
      className="searchable-picker__option menu-item"
      aria-disabled={disabled || undefined}
      aria-describedby={disabled ? `${id}-reason` : undefined}
      aria-selected={selected}
      data-option-id={candidate.id}
      data-active={active || undefined}
      disabled={disabled}
      tabIndex={-1}
      onMouseMove={() => !disabled && onActivate(candidate.id)}
      onClick={() => !disabled && onSelect(candidate.id)}
    >
      {logo ? (
        <img
          src={logo}
          alt=""
          aria-hidden="true"
          data-harness-icon={candidate.icon ?? "generic"}
        />
      ) : null}
      <span className="searchable-picker__option-copy">
        <span>{candidate.label}</span>
        {candidate.secondaryLabel ? (
          <small>{candidate.secondaryLabel}</small>
        ) : null}
      </span>
      {selected && logo ? (
        <Check
          className="searchable-picker__selected-check"
          size={14}
          aria-hidden="true"
        />
      ) : null}
      {candidate.disabledReason ? (
        <span id={`${id}-reason`} className="searchable-picker__reason">
          {candidate.disabledReason}
        </span>
      ) : null}
    </button>
  );
});

function optionId(prefix: string, id: string) {
  return `${prefix}-option-${encodeURIComponent(id)}`;
}

export function SearchablePicker({
  ariaLabel,
  candidates,
  className,
  disabled = false,
  emptyLabel = "Select…",
  footer,
  onOptionRender,
  popoverClassName,
  triggerClassName,
  sectionLabel,
  searchable = true,
  onSelect,
  portalTarget = "document",
  selectedId,
  state,
  stateDetail,
  stateTitle,
}: {
  ariaLabel: string;
  candidates: readonly SearchablePickerOption[];
  className?: string;
  disabled?: boolean;
  emptyLabel?: string;
  footer?: string;
  onOptionRender?: (id: string) => void;
  popoverClassName?: string;
  triggerClassName?: string;
  sectionLabel?: string;
  searchable?: boolean;
  onSelect: (id: string) => void;
  portalTarget?: "closest-dialog" | "document";
  selectedId: string | null;
  state?: "error" | "idle" | "loading";
  stateDetail?: string;
  stateTitle?: string;
}) {
  const id = useId();
  const triggerRef = useRef<HTMLButtonElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const optionRefs = useRef(new Map<string, HTMLButtonElement>());
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState("");
  const [query, setQuery] = useState("");
  const [activeId, setActiveId] = useState<string | null>(null);
  const invalid = candidates.length > SEARCHABLE_PICKER_MAX_CANDIDATES;
  const admittedCandidates = invalid ? EMPTY_CANDIDATES : candidates;
  const selected = useMemo(
    () => admittedCandidates.find((candidate) => candidate.id === selectedId),
    [admittedCandidates, selectedId],
  );
  const visible = useMemo(() => {
    const normalized = query.trim().toLocaleLowerCase();
    return admittedCandidates.filter((candidate) =>
      candidate.label.toLocaleLowerCase().includes(normalized),
    );
  }, [admittedCandidates, query]);
  const rendered = useMemo(
    () => visible.slice(0, SEARCHABLE_PICKER_MAX_RENDERED),
    [visible],
  );
  const selectable = useMemo(
    () => rendered.filter((candidate) => !candidate.disabledReason),
    [rendered],
  );
  const active = useMemo(
    () => selectable.find((candidate) => candidate.id === activeId) ?? null,
    [activeId, selectable],
  );

  const setOptionElement = useCallback(
    (option: string, element: HTMLButtonElement | null) => {
      if (element) optionRefs.current.set(option, element);
      else optionRefs.current.delete(option);
    },
    [],
  );

  useEffect(() => {
    if (!open || disabled) return;
    const timer = window.setTimeout(
      () => setQuery(draft),
      SEARCHABLE_PICKER_DEBOUNCE_MS,
    );
    return () => window.clearTimeout(timer);
  }, [disabled, draft, open]);

  useLayoutEffect(() => {
    if (!open || disabled) return;
    inputRef.current?.focus();
  }, [disabled, open]);
  useLayoutEffect(() => {
    if (!active) return;
    optionRefs.current.get(active.id)?.scrollIntoView?.({ block: "nearest" });
  }, [active]);

  const close = useCallback((reason?: MenuDismissReason) => {
    setOpen(false);
    setActiveId(null);
    if (reason === "escape") triggerRef.current?.focus();
  }, []);
  useLayoutEffect(() => {
    if (!disabled) return;
    queueMicrotask(() => close());
  }, [close, disabled]);
  useLayoutEffect(() => {
    if (!invalid) return;
    queueMicrotask(() => close());
  }, [close, invalid]);
  const moveActive = (direction: number, page = false) => {
    if (selectable.length === 0) return;
    const found = selectable.findIndex(
      (candidate) => candidate.id === activeId,
    );
    const current = found < 0 ? (direction > 0 ? -1 : 0) : found;
    const offset = page ? SEARCHABLE_PICKER_PAGE_SIZE : 1;
    const next = page
      ? Math.max(
          0,
          Math.min(selectable.length - 1, current + direction * offset),
        )
      : (current + direction * offset + selectable.length) % selectable.length;
    setActiveId(selectable[next]?.id ?? null);
  };
  const selectActive = () => {
    if (!active) return;
    onSelect(active.id);
    close();
    triggerRef.current?.focus();
  };
  const selectCandidate = useCallback(
    (next: string) => {
      if (disabled) return;
      onSelect(next);
      close();
      triggerRef.current?.focus();
    },
    [close, disabled, onSelect],
  );
  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (invalid || disabled) return;
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      if (!open) setOpen(true);
      moveActive(event.key === "ArrowDown" ? 1 : -1);
    } else if (event.key === "Home" || event.key === "End") {
      if (selectable.length === 0) return;
      event.preventDefault();
      setActiveId(
        selectable[event.key === "Home" ? 0 : selectable.length - 1]?.id ??
          null,
      );
    } else if (event.key === "PageDown" || event.key === "PageUp") {
      event.preventDefault();
      moveActive(event.key === "PageDown" ? 1 : -1, true);
    } else if (event.key === "Enter") {
      event.preventDefault();
      selectActive();
    } else if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      close("escape");
    } else if (event.key === "Tab") {
      if (portalTarget === "closest-dialog") {
        flushSync(() => close("tab"));
        triggerRef.current?.focus();
        return;
      }
      triggerRef.current?.focus();
      close("tab");
    }
  };

  if (invalid) {
    return (
      <ResourceStateNotice
        state="error"
        title={`${ariaLabel} is unavailable`}
        detail={`This picker accepts at most ${SEARCHABLE_PICKER_MAX_CANDIDATES} candidates.`}
      />
    );
  }

  return (
    <div className={["searchable-picker", className].filter(Boolean).join(" ")}>
      <button
        ref={triggerRef}
        type="button"
        className={["searchable-picker__trigger", triggerClassName]
          .filter(Boolean)
          .join(" ")}
        aria-label={ariaLabel}
        aria-expanded={open && !disabled}
        aria-haspopup="listbox"
        aria-controls={`${id}-listbox`}
        data-selected-id={selectedId ?? ""}
        disabled={disabled}
        onClick={() => (open ? close() : setOpen(true))}
      >
        {selected?.logo || selected?.icon ? (
          <img
            src={selected.logo ?? `/harness-${selected.icon}.svg`}
            alt=""
            aria-hidden="true"
          />
        ) : null}
        <span className="searchable-picker__trigger-label">
          {selected?.triggerLabel ?? selected?.label ?? emptyLabel}
          {selected?.secondaryLabel ? (
            <small>{selected.secondaryLabel}</small>
          ) : null}
        </span>
        <ChevronDown
          className="searchable-picker__chevron"
          size={14}
          aria-hidden="true"
        />
      </button>
      <MenuPopover
        ariaLabel={ariaLabel}
        className={[
          "searchable-picker__popover",
          "menu-popover--editor",
          popoverClassName,
        ]
          .filter(Boolean)
          .join(" ")}
        focusStrategy="none"
        id={`${id}-popover`}
        matchTriggerWidth
        onDismiss={close}
        open={open && !disabled}
        portalTarget={portalTarget}
        role="none"
        triggerRef={triggerRef}
      >
        <input
          ref={inputRef}
          className="searchable-picker__input"
          role="combobox"
          aria-label={ariaLabel}
          aria-activedescendant={active ? optionId(id, active.id) : undefined}
          aria-autocomplete="list"
          readOnly={!searchable}
          aria-controls={`${id}-listbox`}
          aria-expanded={open && !disabled}
          value={draft}
          onChange={(event) => {
            if (!searchable) return;
            setDraft(event.target.value);
            setActiveId(null);
          }}
          onKeyDown={onKeyDown}
        />
        {selected ? (
          <p className="searchable-picker__current">
            Current: {selected.label}
          </p>
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
              active={candidate.id === active?.id}
              candidate={candidate}
              id={optionId(id, candidate.id)}
              onActivate={setActiveId}
              onOptionElement={setOptionElement}
              onRender={onOptionRender}
              onSelect={selectCandidate}
              selected={candidate.id === selectedId}
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
    </div>
  );
}
