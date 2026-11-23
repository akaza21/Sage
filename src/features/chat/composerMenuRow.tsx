import type { ComponentProps, ReactNode } from "react";

/** Shared label, description and trailing columns; callers own the action. */
export function composerMenuRow(
  props: ComponentProps<"button"> & { "data-restored"?: boolean },
  text: string,
  description?: string,
  icon?: ReactNode,
  trailing?: ReactNode,
) {
  return (
    <button
      type="button"
      role="menuitem"
      className="menu-item composer__add-action"
      tabIndex={-1}
      aria-description={description}
      {...props}
    >
      {icon}
      <span>{text}</span>
      {description ? (
        <span className="composer__action-description">{description}</span>
      ) : null}
      {trailing}
    </button>
  );
}
