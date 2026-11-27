import { Fragment, type ReactNode } from "react";
import type { NoticeSeverity } from "./Notice";

export interface StackedNotice {
  /** Unique within the stack; keeps a notice mounted while others come and go. */
  key: string;
  severity: NoticeSeverity;
  node: ReactNode;
  /** A live region kept mounted with nothing to show: it does not make the
   * stack a region of its own. */
  quiet?: boolean;
}

export type StackEntry = StackedNotice | false | null | undefined;

const rank = { blocking: 0, warning: 1, info: 2 };

/**
 * One surface's notices in one place: blocking, then warning, then info,
 * each severity in source order.
 */
export function NoticeStack({
  label,
  notices,
}: {
  label: string;
  notices: readonly StackEntry[];
}) {
  const stack = notices
    .filter((notice): notice is StackedNotice => !!notice)
    .sort((a, b) => rank[a.severity] - rank[b.severity]);
  // Focusable so a stack taller than its cap scrolls from the keyboard.
  const region = stack.some((notice) => !notice.quiet)
    ? { role: "region", "aria-label": label, tabIndex: 0 }
    : null;
  return stack.length ? (
    <div className="notice-stack" {...region}>
      {stack.map((notice) => (
        <Fragment key={notice.key}>{notice.node}</Fragment>
      ))}
    </div>
  ) : null;
}
