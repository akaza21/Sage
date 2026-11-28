import { useId, useRef, useState } from "react";
import type {
  AcpConfigOption,
  AcpConfigValue,
} from "../../domain/acpSessionConfig";
import { flattenChoices } from "../../domain/acpSessionConfigControls";
import { capitalizeLabel } from "../../domain/text";
import { Brain, Check, ChevronRight } from "../../ui/leanIcons";
import { MenuPopover } from "../../ui/MenuPopover";
import "./composer-effort-menu.css";

/**
 * The model popover's "Thinking effort" row: the level the harness reports,
 * opening a submenu of the levels it lists. The harness's default level is
 * marked, so returning to it needs no separate reset.
 */
export default function ComposerEffortMenu({
  option,
  disabled,
  pending,
  onSetOption,
}: {
  option: Extract<AcpConfigOption, { type: "select" }>;
  disabled: boolean;
  pending: boolean;
  onSetOption: (configId: string, value: AcpConfigValue) => Promise<unknown>;
}) {
  const [open, setOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuId = useId();
  const levels = flattenChoices(option);
  const current = levels.find((level) => level.value === option.currentValue);
  if (!current) return null;
  const close = () => {
    setOpen(false);
    triggerRef.current?.focus();
  };
  const select = (value: string) => {
    if (!disabled && !pending && value !== option.currentValue)
      void onSetOption(option.id, { value });
    close();
  };
  return (
    <div className="composer-effort">
      <button
        ref={triggerRef}
        type="button"
        className="composer-effort__trigger"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        aria-disabled={disabled || pending}
        onClick={() => !disabled && !pending && setOpen(!open)}
        onKeyDown={(event) => {
          if (event.key !== "ArrowRight" || disabled || pending) return;
          event.preventDefault();
          event.stopPropagation();
          setOpen(true);
        }}
      >
        <Brain size={14} aria-hidden="true" />
        <span>Thinking effort</span>
        <span className="composer-effort__level">
          {capitalizeLabel(current.name)}
        </span>
        <ChevronRight size={14} aria-hidden="true" />
      </button>
      <MenuPopover
        id={menuId}
        open={open}
        ariaLabel="Thinking effort"
        className="composer-effort__menu"
        initialFocus="selected"
        portalTarget="closest-popover"
        triggerRef={triggerRef}
        onDismiss={(reason) => (reason === "escape" ? close() : setOpen(false))}
      >
        <div
          onKeyDown={(event) => {
            if (event.key !== "ArrowLeft") return;
            event.preventDefault();
            event.stopPropagation();
            close();
          }}
        >
          {levels.map((level) => {
            const checked = level.value === option.currentValue;
            return (
              <button
                key={level.value}
                type="button"
                role="menuitemradio"
                aria-checked={checked}
                className="menu-item composer-effort__item"
                onClick={() => select(level.value)}
              >
                <span>{capitalizeLabel(level.name)}</span>
                {level.value === option.defaultValue ? (
                  <span className="menu-description">Default</span>
                ) : null}
                {checked ? <Check size={14} aria-hidden="true" /> : null}
              </button>
            );
          })}
        </div>
      </MenuPopover>
    </div>
  );
}
