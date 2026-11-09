import {
  useEffect,
  useCallback,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  useSyncExternalStore,
  type ComponentProps,
  type ReactNode,
} from "react";
import type { HarnessBrand } from "../domain/harness";
import "../styles/searchable-picker.css";
import { useColdOpen } from "./coldOpen";
import { ChevronDown } from "./leanIcons";
import type { MenuDismissReason } from "./MenuPopover";
import type { MenuPlacement } from "./menuPopoverPosition";
import { PickerLogo } from "./PickerLogo";
import { ResourceStateNotice } from "./ResourceStateNotice";
import {
  loadedSearchablePickerPopover,
  preloadSearchablePicker,
  SEARCHABLE_PICKER_MAX_CANDIDATES,
  subscribeSearchablePicker,
} from "./searchablePickerLoad";
import type * as PopoverChunk from "./SearchablePickerPopover";
import { whenIdle } from "./whenIdle";

export interface SearchablePickerOption {
  id: string;
  label: string;
  subtitle?: string;
  accessibleLabel?: string;
  triggerLabel?: string;
  disabledReason?: string;
  icon?: HarnessBrand | "generic";
  logo?: string;
  /** A secondary action rendered beside the option, e.g. "Sign in". */
  action?: { label: string; accessibleLabel?: string; onAction: () => void };
}

const preload = () => void preloadSearchablePicker().catch(() => undefined);

type PopoverProps = ComponentProps<typeof PopoverChunk.SearchablePickerPopover>;
const renderPopover = (
  Loaded: typeof PopoverChunk.SearchablePickerPopover,
  props: PopoverProps,
) => <Loaded {...props} />;

/** Renders the open list; its picker mounts it once the chunk is in. */
function PopoverSlot(props: PopoverProps) {
  const Loaded = loadedSearchablePickerPopover();
  return Loaded ? renderPopover(Loaded, props) : null;
}

/**
 * The open list once its chunk has arrived, whichever picker asked for it.
 * It is fetched when the window is idle; a picker opened before then asks
 * again, and closes if that fails so the next open retries.
 */
function usePopoverLoad(wanted: boolean, onFailure: () => void) {
  const Loaded = useSyncExternalStore(
    subscribeSearchablePicker,
    loadedSearchablePickerPopover,
  );
  useEffect(
    () => (loadedSearchablePickerPopover() ? undefined : whenIdle(preload)),
    [],
  );
  useEffect(() => {
    if (!wanted || Loaded) return;
    let live = true;
    void preloadSearchablePicker().catch(() => live && onFailure());
    return () => {
      live = false;
    };
  }, [Loaded, onFailure, wanted]);
  return Loaded;
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
  popoverPlacement,
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
  /** Where the list opens relative to its trigger; see MenuPlacement. */
  popoverPlacement?: MenuPlacement;
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
  const [internalOpen, setInternalOpen] = useState(false);
  const open = controlledOpen ?? internalOpen;
  const setOpen = onOpenChange ?? setInternalOpen;
  const expanded = open && !disabled;
  // Once opened, the list stays mounted; the typed query lives here, so it
  // also outlives a close and the too-many-candidates notice.
  const [opened, setOpened] = useState(expanded);
  if (expanded && !opened) setOpened(true);
  const [draft, setDraft] = useState("");
  const [loadFailed, setLoadFailed] = useState(false);
  if (expanded && loadFailed) setLoadFailed(false);
  const invalid = candidates.length > SEARCHABLE_PICKER_MAX_CANDIDATES;
  const selected = invalid
    ? undefined
    : candidates.find((candidate) => candidate.id === selectedId);
  const selectedLabel = selected?.triggerLabel ?? selected?.label;

  const close = useCallback(
    (reason?: MenuDismissReason) => {
      setOpen(false);
      if (reason === "escape" || (!searchable && reason === "tab"))
        triggerRef.current?.focus();
    },
    [searchable, setOpen],
  );
  const failLoad = useCallback(() => {
    setLoadFailed(true);
    close();
  }, [close]);
  const Loaded = usePopoverLoad(expanded, failLoad);
  const coldOpen = useColdOpen({
    dismiss: close,
    loaded: Boolean(Loaded),
    onType: searchable
      ? (text) => setDraft((current) => current + text)
      : undefined,
    open: expanded,
    triggerRef,
  });
  useLayoutEffect(() => {
    if (disabled || invalid) queueMicrotask(() => close());
  }, [close, disabled, invalid]);

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
        aria-description={selectedLabel}
        title={stateTitle ?? selectedLabel}
        aria-expanded={expanded}
        aria-haspopup="listbox"
        aria-controls={Loaded && expanded ? `${id}-listbox` : undefined}
        role={triggerRole}
        data-selected-id={selectedId ?? ""}
        disabled={disabled}
        onClick={() => (open ? close() : setOpen(true))}
        onPointerEnter={preload}
        onPointerDown={preload}
        onFocus={preload}
        onKeyUp={coldOpen.onKeyUp}
        onKeyDown={(event) => {
          if (coldOpen.onKeyDown(event) || searchable || !open) return;
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
          {selectedLabel ?? emptyLabel}
        </span>
        <ChevronDown
          className="searchable-picker__chevron"
          size={14}
          aria-hidden="true"
        />
      </button>
      {loadFailed ? (
        <ResourceStateNotice
          state="error"
          title={`${ariaLabel} could not be loaded.`}
          detail="Open it again to retry."
        />
      ) : null}
      {opened && Loaded ? (
        <PopoverSlot
          ariaLabel={ariaLabel}
          candidates={candidates}
          close={close}
          disabled={disabled}
          draft={draft}
          expanded={expanded}
          focusMoved={coldOpen.focusMoved}
          footer={footer}
          id={id}
          onOptionRender={onOptionRender}
          onSelect={onSelect}
          open={open}
          popoverClassName={popoverClassName}
          popoverGap={popoverGap}
          popoverPlacement={popoverPlacement}
          portalTarget={portalTarget}
          searchable={searchable}
          sectionLabel={sectionLabel}
          selected={selected}
          selectedId={selectedId}
          setDraft={setDraft}
          setOpen={setOpen}
          state={state}
          stateDetail={stateDetail}
          stateTitle={stateTitle}
          triggerRef={triggerRef}
        />
      ) : null}
    </div>
  );
}
