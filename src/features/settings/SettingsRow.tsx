import type { ReactNode } from "react";

export function SettingsRow({
  title,
  description,
  notice,
  children,
}: {
  title: string;
  description?: string;
  /** A problem with this row's setting, shown beside the setting it concerns. */
  notice?: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className="agent-settings__row">
      <div className="agent-settings__copy">
        <strong>{title}</strong>
        {description ? <p>{description}</p> : null}
        {notice}
      </div>
      <div className="agent-settings__control">{children}</div>
    </div>
  );
}
