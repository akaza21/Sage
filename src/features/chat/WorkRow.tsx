import type { ReactNode } from "react";

/** One row of the Worked groups; the groups animate and space these. */
export function WorkRow({
  afterMessage = false,
  children,
}: {
  afterMessage?: boolean;
  children: ReactNode;
}) {
  return (
    <div
      className={`chat-transcript-work__row${afterMessage ? " chat-transcript-work__row--after-message" : ""}`}
    >
      {children}
    </div>
  );
}
