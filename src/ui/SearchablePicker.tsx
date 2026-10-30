import {
  useEffect,
  useCallback,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import { flushSync } from "react-dom";
import { ChevronDown } from "./leanIcons";
import { PickerOption, PickerLogo } from "./PickerOption";

import { MenuPopover, type MenuDismissReason } from "./MenuPopover";
import { ResourceStateNotice } from "./ResourceStateNotice";

export const SEARCHABLE_PICKER_MAX_CANDIDATES = 500;
export const SEARCHABLE_PICKER_MAX_RENDERED = 200;
export const SEARCHABLE_PICKER_DEBOUNCE_MS = 50;
export const SEARCHABLE_PICKER_PAGE_SIZE = 10;

export interface SearchablePickerOption {
  id: string;
  label: string;
  subtitle?: string;
  accessibleLabel?: string;
  triggerLabel?: string;
  disabledReason?: string;
  icon?: "claude" | "codex" | "opencode";
  logo?: string;
  /** A secondary action rendered beside the option, e.g. "Sign in". */
  action?: { label: string; accessibleLabel?: string; onAction: () => void };
}

const EMPTY_CANDIDATES: readonly SearchablePickerOption[] = [];

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
  open: controlledOpen,
  onOpenChange,
  popoverClassName,
  popoverGap,
  triggerClassName,
  triggerRole,
  triggerIcon,
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
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  popoverClassName?: string;
  popoverGap?: number;
  triggerClassName?: string;
  triggerRole?: "menuitem";
  triggerIcon?: ReactNode;
  sectionLabel?: string;
  searchable?: boolean;
  onSelect: (id: string) => void;
  portalTarget?: "closest-dialog" | "closest-popover" | "document";
  selectedId: string | null;
  state?: "error" | "idle" | "loading";
  stateDetail?: string;
  stateTitle?: string;
}) {
  const id = useId();
  const triggerRef = useRef<HTMLButtonElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const optionRefs = useRef(new Map<string, HTMLButtonElement>());
  const [internalOpen, setInternalOpen] = useState(false);
  const open = controlledOpen ?? internalOpen;
  const setOpen = onOpenChange ?? setInternalOpen;
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
    if (!open || disabled || !searchable) return;
    inputRef.current?.focus();
  }, [disabled, open, searchable]);
  useLayoutEffect(() => {
    if (!active) return;
    optionRefs.current.get(active.id)?.scrollIntoView?.({ block: "nearest" });
  }, [active]);

  const close = useCallback(
    (reason?: MenuDismissReason) => {
      setOpen(false);
      setActiveId(null);
      if (reason === "escape" || (!searchable && reason === "tab"))
        triggerRef.current?.focus();
    },
    [searchable, setOpen],
  );
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
      if (portalTarget !== "document") {
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
        aria-description={selected?.triggerLabel ?? selected?.label}
        title={stateTitle ?? selected?.triggerLabel ?? selected?.label}
        aria-expanded={open && !disabled}
        aria-haspopup="listbox"
        aria-controls={`${id}-listbox`}
        role={triggerRole}
        data-selected-id={selectedId ?? ""}
        disabled={disabled}
        onClick={() => (open ? close() : setOpen(true))}
        onKeyDown={(event) => {
          if (searchable || !open) return;
          if (event.key === "Escape") {
            event.preventDefault();
            event.stopPropagation();
            close("escape");
          } else if (event.key === "Tab") close("tab");
        }}
      >
        {selected?.logo || selected?.icon ? (
          <PickerLogo logo={selected.logo} icon={selected.icon} />
        ) : (
          triggerIcon
        )}
        <span className="searchable-picker__trigger-label">
          {selected?.triggerLabel ?? selected?.label ?? emptyLabel}
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
        focusStrategy={searchable ? "none" : "roving"}
        focusKey={
          searchable
            ? undefined
            : selectable.map((candidate) => candidate.id).join("\0")
        }
        initialFocus="selected"
        id={`${id}-popover`}
        matchTriggerWidth
        gap={popoverGap}
        onDismiss={close}
        open={open && !disabled}
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
            aria-expanded={open && !disabled}
            value={draft}
            onChange={(event) => {
              setDraft(event.target.value);
              setActiveId(null);
            }}
            onKeyDown={onKeyDown}
          />
        ) : null}
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
          {rendered.map((candidate) => {
            const option = (
              <PickerOption
                key={candidate.id}
                active={candidate.id === active?.id}
                candidate={candidate}
                id={optionId(id, candidate.id)}
                onActivate={searchable ? setActiveId : undefined}
                onOptionElement={setOptionElement}
                onRender={onOptionRender}
                onSelect={selectCandidate}
                selected={candidate.id === selectedId}
              />
            );
            const action = candidate.action;
            if (!action) return option;
            return (
              <div
                key={candidate.id}
                className="searchable-picker__option-row"
                role="none"
              >
                {option}
                <button
                  type="button"
                  className="searchable-picker__option-action"
                  aria-label={action.accessibleLabel ?? action.label}
                  disabled={disabled}
                  onClick={(event) => {
                    event.stopPropagation();
                    action.onAction();
                  }}
                >
                  {action.label}
                </button>
              </div>
            );
          })}
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
