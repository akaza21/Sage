import {
  useContext,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type ReactNode,
  type RefObject,
} from "react";
import { createPortal } from "react-dom";
import { useNativePageLayer } from "./nativePageLayers";
import {
  exitPopoverDialogOnTab,
  focusPopoverDialog,
} from "./menuPopoverDialogFocus";
import {
  inFixedContainingBlock,
  type MenuPlacement,
  menuPopoverPosition,
  PopoverPlacementContext,
} from "./menuPopoverPosition";

const ITEM_SELECTOR = [
  '[role="menuitem"]',
  '[role="menuitemradio"]',
  '[role="menuitemcheckbox"]',
  '[role="option"]',
].join(",");
const POPOVER_GAP = 4;
/** The style keys the placement functions set. */
const POSITION_KEYS = [
  "left",
  "top",
  "minWidth",
  "maxWidth",
  "maxHeight",
  "visibility",
] as const;
const MAX_POSITION_UPDATES_PER_FRAME = 4;

type InitialFocus = "first" | "last" | "selected";
type PopoverRole = "menu" | "listbox" | "dialog" | "none";
type FocusStrategy = "none" | "roving";
type PortalTarget = "closest-dialog" | "closest-popover" | "document";
export type MenuDismissReason = "escape" | "outside" | "tab";

function menuItems(container: HTMLElement) {
  return Array.from(
    container.querySelectorAll<HTMLElement>(ITEM_SELECTOR),
  ).filter((item) => item.closest(".menu-popover") === container);
}

function enabledMenuItems(container: HTMLElement) {
  return menuItems(container).filter((item) => !item.hasAttribute("disabled"));
}

function activateMenuItem(container: HTMLElement, next: number) {
  const all = menuItems(container);
  const enabled = enabledMenuItems(container);
  all.forEach((item) => {
    item.tabIndex = -1;
  });
  const item = enabled[next];
  if (!item) {
    if (container.getAttribute("role") === "menu") {
      container.tabIndex = -1;
      container.focus();
    }
    return;
  }
  item.tabIndex = 0;
  item.focus();
}

function selectedItemIndex(items: HTMLElement[]) {
  const selected = items.findIndex(
    (item) =>
      item.getAttribute("aria-selected") === "true" ||
      item.getAttribute("aria-checked") === "true",
  );
  return selected >= 0 ? selected : 0;
}

export function MenuPopover({
  ariaLabel,
  ariaDescribedBy,
  children,
  className,
  focusKey,
  focusStrategy = "roving",
  id,
  initialFocus = "first",
  keepMounted = false,
  matchTriggerWidth = false,
  gap = POPOVER_GAP,
  onDismiss,
  open,
  placement: preferredPlacement = "below-end",
  positionAt,
  portalTarget = "document",
  restoreFocusOnTab = false,
  role = "menu",
  trapFocus = false,
  triggerRef,
}: {
  ariaLabel: string;
  ariaDescribedBy?: string;
  children: ReactNode;
  className?: string;
  focusKey?: string | number | boolean;
  focusStrategy?: FocusStrategy;
  id: string;
  initialFocus?: InitialFocus;
  keepMounted?: boolean;
  matchTriggerWidth?: boolean;
  gap?: number;
  onDismiss: (reason: MenuDismissReason) => void;
  open: boolean;
  /** Top-level menus only; nested menus follow their parent. */
  placement?: MenuPlacement;
  /**
   * Places the popover itself, for example at a right-click's pointer
   * (`pointerPopoverPosition`), instead of beside the trigger. The trigger
   * still takes the focus back when the menu closes.
   */
  positionAt?: (popover: HTMLElement) => CSSProperties;
  portalTarget?: PortalTarget;
  restoreFocusOnTab?: boolean;
  role?: PopoverRole;
  trapFocus?: boolean;
  triggerRef: RefObject<HTMLElement | null>;
}) {
  const popoverRef = useRef<HTMLDivElement>(null);
  const parentPlacement = useContext(PopoverPlacementContext);
  const originRef = parentPlacement?.originRef ?? triggerRef;
  const typeaheadRef = useRef("");
  const typeaheadTimerRef = useRef<number | null>(null);
  const [portalRoot, setPortalRoot] = useState<HTMLElement>(
    () => document.body,
  );
  useNativePageLayer(popoverRef, { open, portalRoot });
  const [position, setPosition] = useState<CSSProperties>(() =>
    focusStrategy === "none" ? {} : { visibility: "hidden" },
  );
  const positionRef = useRef(position);
  const settleRef = useRef({ frame: 0, count: 0 });
  const measureRef = useRef<() => void>(() => {});
  const hiddenTriggerRef = useRef<() => void>(() => {});
  const placement = useMemo(
    () => ({ originRef, position }),
    [originRef, position],
  );

  useLayoutEffect(() => {
    if (!open) return;
    const popover = popoverRef.current;
    const trigger = triggerRef.current;
    if (!popover || !trigger) return;

    const updatePosition = () => {
      const next = inFixedContainingBlock(
        positionAt
          ? positionAt(popover)
          : menuPopoverPosition(
              trigger,
              popover,
              originRef.current ?? trigger,
              gap,
              matchTriggerWidth,
              preferredPlacement,
            ),
        popover,
      );
      const current = positionRef.current;
      if (POSITION_KEYS.every((key) => current[key] === next[key])) return;
      // Two placements can each make the other the better fit (a flip that
      // changes the room left for the list), so bound synchronous
      // re-measures per frame instead of looping until React gives up.
      const guard = settleRef.current;
      if (guard.count >= MAX_POSITION_UPDATES_PER_FRAME) return;
      guard.count += 1;
      guard.frame ||= requestAnimationFrame(() => {
        guard.frame = 0;
        guard.count = 0;
      });
      positionRef.current = next;
      setPosition(next);
    };

    measureRef.current = updatePosition;
    hiddenTriggerRef.current = () => onDismiss("outside");
    updatePosition();
  });

  useLayoutEffect(() => {
    if (!open) return;
    const popover = popoverRef.current;
    const trigger = triggerRef.current;
    if (!popover || !trigger) return;
    // A trigger that stops rendering (for example a compacted composer
    // control) leaves no anchor, so the popover closes instead of drifting.
    const updatePosition = () =>
      typeof trigger.checkVisibility === "function" &&
      !trigger.checkVisibility()
        ? hiddenTriggerRef.current()
        : measureRef.current();
    const resizeObserver =
      typeof ResizeObserver === "undefined"
        ? null
        : new ResizeObserver(updatePosition);
    resizeObserver?.observe(popover);
    const pane = trigger.closest(".agent-chat-pane, .agent-chat-center");
    for (
      let ancestor: HTMLElement | null = trigger;
      ancestor;
      ancestor = ancestor.parentElement
    ) {
      resizeObserver?.observe(ancestor);
      if (ancestor === pane || ancestor === portalRoot) break;
    }
    window.addEventListener("resize", updatePosition);
    window.addEventListener("scroll", updatePosition, true);
    return () => {
      window.removeEventListener("resize", updatePosition);
      window.removeEventListener("scroll", updatePosition, true);
      resizeObserver?.disconnect();
    };
  }, [open, portalRoot, triggerRef]);

  useLayoutEffect(() => {
    if (!open) return;
    const nextRoot =
      portalTarget !== "document"
        ? (triggerRef.current?.closest<HTMLElement>(
            portalTarget === "closest-popover"
              ? ".menu-popover"
              : 'dialog, [role="dialog"]',
          ) ?? document.body)
        : document.body;
    setPortalRoot((current) => (current === nextRoot ? current : nextRoot));
  }, [open, portalTarget, triggerRef]);

  useLayoutEffect(() => {
    if (!open || portalTarget === "document") return;
    popoverRef.current
      ?.querySelector<HTMLElement>('[role="combobox"]')
      ?.focus();
  }, [open, portalRoot, portalTarget]);

  useLayoutEffect(() => {
    if (!open || focusStrategy === "none") return;
    const popover = popoverRef.current;
    if (!popover) return;
    if (role === "dialog") {
      focusPopoverDialog(popover);
      return;
    }
    const items = enabledMenuItems(popover);
    const next =
      initialFocus === "last"
        ? items.length - 1
        : initialFocus === "selected"
          ? selectedItemIndex(items)
          : 0;
    activateMenuItem(popover, next);
    queueMicrotask(() => {
      if (popover.isConnected && !popover.hidden) {
        activateMenuItem(popover, next);
      }
    });
    // portalRoot: a nested menu first renders into the body, then moves into
    // its parent popover; the move remounts it, so focus is placed again.
  }, [focusKey, focusStrategy, initialFocus, open, role, portalRoot]);

  useEffect(() => {
    if (!open) return;
    const dismissOutside = (event: PointerEvent) => {
      const target = event.target as Node | null;
      if (
        !target ||
        popoverRef.current?.contains(target) ||
        triggerRef.current?.contains(target)
      ) {
        return;
      }
      onDismiss("outside");
    };
    document.addEventListener("pointerdown", dismissOutside, true);
    return () =>
      document.removeEventListener("pointerdown", dismissOutside, true);
  }, [onDismiss, open, triggerRef]);

  useEffect(
    () => () => {
      if (typeaheadTimerRef.current !== null) {
        window.clearTimeout(typeaheadTimerRef.current);
      }
    },
    [],
  );

  if (!open && !keepMounted) return null;

  const moveFocus = (event: KeyboardEvent<HTMLDivElement>) => {
    if (focusStrategy === "none") return;
    const popover = event.currentTarget;
    const items = enabledMenuItems(popover);
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      triggerRef.current?.focus();
      onDismiss("escape");
      return;
    }
    if (event.key === "Tab") {
      if (role === "dialog") {
        exitPopoverDialogOnTab(event, triggerRef.current, () =>
          onDismiss("tab"),
        );
        return;
      }
      if (trapFocus && items.length > 0) {
        event.preventDefault();
        const current = Math.max(
          0,
          items.indexOf(document.activeElement as HTMLElement),
        );
        activateMenuItem(
          popover,
          (current + (event.shiftKey ? -1 : 1) + items.length) % items.length,
        );
      } else {
        if (restoreFocusOnTab) {
          event.preventDefault();
          triggerRef.current?.focus();
        }
        onDismiss("tab");
      }
      return;
    }
    if (role === "dialog") return;
    if (["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
      if (items.length === 0) return;
      event.preventDefault();
      event.stopPropagation();
      const current = Math.max(
        0,
        items.indexOf(document.activeElement as HTMLElement),
      );
      const next =
        event.key === "Home"
          ? 0
          : event.key === "End"
            ? items.length - 1
            : event.key === "ArrowDown"
              ? (current + 1) % items.length
              : (current - 1 + items.length) % items.length;
      activateMenuItem(popover, next);
      return;
    }
    if (
      event.key.length !== 1 ||
      event.altKey ||
      event.ctrlKey ||
      event.metaKey
    ) {
      return;
    }
    typeaheadRef.current += event.key.toLocaleLowerCase();
    if (typeaheadTimerRef.current !== null) {
      window.clearTimeout(typeaheadTimerRef.current);
    }
    typeaheadTimerRef.current = window.setTimeout(() => {
      typeaheadRef.current = "";
      typeaheadTimerRef.current = null;
    }, 500);
    const match = items.findIndex((item) =>
      (item.textContent ?? "")
        .trim()
        .toLocaleLowerCase()
        .startsWith(typeaheadRef.current),
    );
    if (match >= 0) activateMenuItem(popover, match);
  };

  return createPortal(
    <PopoverPlacementContext value={placement}>
      <div
        ref={popoverRef}
        id={id}
        className={[
          "menu-popover",
          position.maxHeight === undefined ? null : "menu-popover--bounded",
          className,
        ]
          .filter(Boolean)
          .join(" ")}
        role={role === "none" ? undefined : role}
        tabIndex={role === "dialog" ? -1 : undefined}
        aria-label={role === "none" ? undefined : ariaLabel}
        aria-describedby={ariaDescribedBy}
        hidden={!open}
        style={position}
        onKeyDown={moveFocus}
        onFocus={(event) => {
          if (focusStrategy === "none") return;
          const item = (event.target as HTMLElement).closest<HTMLElement>(
            ITEM_SELECTOR,
          );
          if (!item || item.hasAttribute("disabled")) return;
          const items = enabledMenuItems(event.currentTarget);
          const index = items.indexOf(item);
          if (index >= 0) activateMenuItem(event.currentTarget, index);
        }}
      >
        {children}
      </div>
    </PopoverPlacementContext>,
    portalRoot,
  );
}
