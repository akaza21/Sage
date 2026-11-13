import type { ReactNode } from "react";

/** One setting in an Agent settings card: its copy, then its control. */
export function SettingsRow({
  title,
  description,
  children,
}: {
  title: string;
  description?: string;
  children: ReactNode;
}) {
  return (
    <div className="agent-settings__row">
      <div className="agent-settings__copy">
        <strong>{title}</strong>
        {description ? <p>{description}</p> : null}
      </div>
      <div className="agent-settings__control">{children}</div>
    </div>
  );
}
