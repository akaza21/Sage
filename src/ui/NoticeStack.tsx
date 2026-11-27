import type { ReactElement, ReactNode } from "react";
import type { NoticeSeverity } from "./Notice";

interface StackedProps {
  severity?: NoticeSeverity;
  /** A live region or description kept mounted with nothing visible: it
   * does not make the stack a region of its own. */
  quiet?: boolean;
}

/** A stack entry that is not a `Notice`; unmarked entries are info. */
export function Stacked({ children }: StackedProps & { children: ReactNode }) {
  return children;
}

/** A keyed `Notice` or `Stacked` element, or nothing. */
export type StackEntry = ReactElement<StackedProps> | false | null | undefined;

// Unmarked entries and info share the last rank.
const rank: Partial<Record<NoticeSeverity, number>> = {
  blocking: 1,
  warning: 2,
};
const rankOf = (notice: ReactElement<StackedProps>) =>
  rank[notice.props.severity!] || 3;

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
    .filter((notice): notice is ReactElement<StackedProps> => !!notice)
    .sort((a, b) => rankOf(a) - rankOf(b));
  // Focusable so a stack taller than its cap scrolls from the keyboard.
  const region = stack.some((notice) => !notice.props.quiet)
    ? { role: "region", "aria-label": label, tabIndex: 0 }
    : null;
  return stack.length ? (
    <div className="notice-stack" {...region}>
      {stack}
    </div>
  ) : null;
}
