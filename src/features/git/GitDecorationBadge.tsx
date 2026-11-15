import type { GitDecoration } from "./gitDecorations";
import "./git-decorations.css";

/** The row's accessible name carries the meaning, so the badge is hidden. */
export function GitDecorationBadge({
  decoration,
}: {
  decoration: GitDecoration;
}) {
  return (
    <span
      className={`git-badge git-tone--${decoration.tone}`}
      title={decoration.label}
      aria-hidden="true"
    >
      {decoration.badge}
    </span>
  );
}
