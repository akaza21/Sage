import type { CSSProperties } from "react";

import type { AcpItem } from "../../domain/acp";

type UsagePayload = Extract<AcpItem, { kind: "usage" }>["payload"];
type UsageStyle = CSSProperties & { "--composer-context-usage": string };

function compactTokenCount(tokens: number): string {
  if (tokens >= 1_000 && tokens % 1_000 === 0) {
    return `${tokens / 1_000}K`;
  }
  return tokens.toLocaleString();
}

function UsageContext({ usage }: { usage: UsagePayload }) {
  const total = usage.totalTokens;
  if (total === null) return null;
  const formattedTotal = total.toLocaleString();
  const capacity = usage.contextWindow;
  if (
    capacity === undefined ||
    !Number.isSafeInteger(capacity) ||
    capacity <= 0 ||
    capacity > 0xffff_ffff
  ) {
    return (
      <span className="composer__context-item">
        Reported usage · {formattedTotal} tokens
      </span>
    );
  }

  const progress = Math.min(Math.max(total, 0), capacity);
  const formattedCapacity = capacity.toLocaleString();
  const fill = `${(progress / capacity) * 100}%`;
  return (
    <span className="composer__context-item composer__context-item--usage">
      <span className="composer__context-usage-label">
        {compactTokenCount(total)} context
      </span>
      <span
        className="composer__context-usage"
        title={`${formattedTotal} / ${formattedCapacity} tokens`}
        role="progressbar"
        aria-label="Reported usage"
        aria-valuemin={0}
        aria-valuemax={capacity}
        aria-valuenow={progress}
        aria-valuetext={`${formattedTotal} / ${formattedCapacity} tokens used`}
        style={{ "--composer-context-usage": fill } as UsageStyle}
      >
        <span className="composer__context-usage-fill" aria-hidden="true" />
      </span>
    </span>
  );
}

export function ComposerContext({
  usage,
}: {
  usage: Extract<AcpItem, { kind: "usage" }>["payload"] | null;
}) {
  return (
    <p className="composer__context" aria-label="Composer context">
      {usage ? <UsageContext usage={usage} /> : null}
    </p>
  );
}
