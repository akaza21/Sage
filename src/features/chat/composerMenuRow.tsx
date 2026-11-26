import type { ComponentProps, ReactNode } from "react";

export interface ComposerMenuRowContent {
  text: string;
  description?: string;
  icon?: ReactNode;
  trailing?: ReactNode;
}

/** Shared label, description and trailing columns; callers own the action. */
export function composerMenuRow(
  props: ComponentProps<"button"> & { "data-restored"?: boolean },
  { text, description, icon, trailing }: ComposerMenuRowContent,
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
        <span className="menu-description" title={description}>
          {description}
        </span>
      ) : null}
      {trailing}
    </button>
  );
}
