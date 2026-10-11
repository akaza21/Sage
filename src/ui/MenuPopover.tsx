import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type ReactNode,
  type RefObject,
} from "react";
import { createPortal } from "react-dom";

const ITEM_SELECTOR = [
  '[role="menuitem"]',
  '[role="menuitemradio"]',
  '[role="menuitemcheckbox"]',
  '[role="option"]',
].join(",");
const VIEWPORT_GUTTER = 8;
const POPOVER_GAP = 4;

type InitialFocus = "first" | "last" | "selected";
type PopoverRole = "menu" | "listbox" | "dialog" | "none";
type FocusStrategy = "none" | "roving";
type PortalTarget = "closest-dialog" | "document";
export type MenuDismissReason = "escape" | "outside" | "tab";

function menuItems(container: HTMLElement) {
  return Array.from(container.querySelectorAll<HTMLElement>(ITEM_SELECTOR));
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
  if (!item) return;
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
  portalTarget = "document",
  role = "menu",
  trapFocus = false,
  triggerRef,
}: {
  ariaLabel: string;
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
  portalTarget?: PortalTarget;
  role?: PopoverRole;
  trapFocus?: boolean;
  triggerRef: RefObject<HTMLElement | null>;
}) {
  const popoverRef = useRef<HTMLDivElement>(null);
  const typeaheadRef = useRef("");
  const typeaheadTimerRef = useRef<number | null>(null);
  const [portalRoot, setPortalRoot] = useState<HTMLElement>(
    () => document.body,
  );
  const [position, setPosition] = useState<CSSProperties>(() =>
    focusStrategy === "none" ? {} : { visibility: "hidden" },
  );

  useLayoutEffect(() => {
    if (!open) return;
    const popover = popoverRef.current;
    const trigger = triggerRef.current;
    if (!popover || !trigger) return;

    const updatePosition = () => {
      const anchor = trigger.getBoundingClientRect();
      const surface = popover.getBoundingClientRect();
      const width = Math.max(
        surface.width,
        matchTriggerWidth ? anchor.width : 0,
      );
      const roomBelow = window.innerHeight - anchor.bottom - VIEWPORT_GUTTER;
      const roomAbove = anchor.top - VIEWPORT_GUTTER;
      const placeAbove = surface.height > roomBelow && roomAbove > roomBelow;
      const top = placeAbove
        ? Math.max(VIEWPORT_GUTTER, anchor.top - surface.height - gap)
        : Math.min(
            anchor.bottom + gap,
            window.innerHeight - surface.height - VIEWPORT_GUTTER,
          );
      const preferredLeft = anchor.right - width;
      const left = Math.min(
        Math.max(VIEWPORT_GUTTER, preferredLeft),
        Math.max(VIEWPORT_GUTTER, window.innerWidth - width - VIEWPORT_GUTTER),
      );

      setPosition({
        left,
        minWidth: matchTriggerWidth ? anchor.width : undefined,
        top: Math.max(VIEWPORT_GUTTER, top),
        visibility: "visible",
      });
    };

    updatePosition();
    const resizeObserver =
      typeof ResizeObserver === "undefined"
        ? null
        : new ResizeObserver(updatePosition);
    resizeObserver?.observe(popover);
    resizeObserver?.observe(trigger);
    window.addEventListener("resize", updatePosition);
    window.addEventListener("scroll", updatePosition, true);
    return () => {
      window.removeEventListener("resize", updatePosition);
      window.removeEventListener("scroll", updatePosition, true);
      resizeObserver?.disconnect();
    };
  }, [focusKey, gap, matchTriggerWidth, open, portalRoot, triggerRef]);

  useLayoutEffect(() => {
    if (!open) return;
    const nextRoot =
      portalTarget === "closest-dialog"
        ? (triggerRef.current?.closest<HTMLElement>(
            'dialog, [role="dialog"]',
          ) ?? document.body)
        : document.body;
    setPortalRoot((current) => (current === nextRoot ? current : nextRoot));
  }, [open, portalTarget, triggerRef]);

  useLayoutEffect(() => {
    if (!open || portalTarget !== "closest-dialog") return;
    popoverRef.current
      ?.querySelector<HTMLElement>('[role="combobox"]')
      ?.focus();
  }, [open, portalRoot, portalTarget]);

  useLayoutEffect(() => {
    if (!open || focusStrategy === "none") return;
    const popover = popoverRef.current;
    if (!popover) return;
    if (role === "dialog") {
      const focusDialog = () => {
        if (popover.isConnected && !popover.hidden) {
          (
            popover.querySelector<HTMLElement>(
              "button:not(:disabled), input:not(:disabled)",
            ) ?? popover
          ).focus();
        }
      };
      focusDialog();
      queueMicrotask(focusDialog);
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
  }, [focusKey, focusStrategy, initialFocus, open, role]);

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
      if (role === "dialog") return;
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
    <div
      ref={popoverRef}
      id={id}
      className={["menu-popover", className].filter(Boolean).join(" ")}
      role={role === "none" ? undefined : role}
      tabIndex={role === "dialog" ? -1 : undefined}
      aria-label={role === "none" ? undefined : ariaLabel}
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
    </div>,
    portalRoot,
  );
}
