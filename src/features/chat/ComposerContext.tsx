import type { CSSProperties } from "react";
import "../../styles/composer-context.css";

import type { ChatUsage } from "./acp/chatUsage";

type UsagePayload = ChatUsage;
type UsageStyle = CSSProperties & { "--composer-context-usage": string };

function compactTokenCount(tokens: number): string {
  if (tokens >= 1_000 && tokens % 1_000 === 0) {
    return `${tokens / 1_000}K`;
  }
  return tokens.toLocaleString();
}

function UsageContext({ usage }: { usage: UsagePayload }) {
  const total = usage.totalTokens;
  const formattedTotal = total.toLocaleString();
  const capacity = usage.contextWindow;
  if (
    capacity === undefined ||
    !Number.isSafeInteger(capacity) ||
    capacity <= 0 ||
    capacity > 0xffff_ffff
  ) {
    return null;
  }

  const progress = Math.min(Math.max(total, 0), capacity);
  const formattedCapacity = capacity.toLocaleString();
  const fill = `${(progress / capacity) * 100}%`;
  const tooltip = `${Math.round(total / 1_000)}k / ${Math.round(capacity / 1_000)}k tokens · ${Math.round((total / capacity) * 100)}%`;
  return (
    <span className="composer__context-item composer__context-item--usage">
      <span className="composer__context-usage-label">
        {compactTokenCount(total)} context
      </span>
      <span
        className="composer__context-usage"
        title={tooltip}
        role="progressbar"
        aria-label={`${formattedTotal} used / ${formattedCapacity} total tokens`}
        tabIndex={0}
        aria-valuemin={0}
        aria-valuemax={capacity}
        aria-valuenow={progress}
        aria-valuetext={`${formattedTotal} / ${formattedCapacity} tokens used`}
        style={{ "--composer-context-usage": fill } as UsageStyle}
      />
    </span>
  );
}

export function ComposerContext({ usage }: { usage: ChatUsage | null }) {
  if (
    !usage ||
    !Number.isSafeInteger(usage.contextWindow) ||
    !usage.contextWindow ||
    usage.contextWindow <= 0
  )
    return null;
  return (
    <p className="composer__context" aria-label="Composer context">
      <UsageContext usage={usage} />
    </p>
  );
}
