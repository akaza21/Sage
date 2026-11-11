import { useId, useRef, useState, type CSSProperties } from "react";
import type { AcpUsageRequest } from "../../domain/acpUsage";
import type { ChatUsage } from "./acp/chatUsage";
import type * as LimitsChunk from "./ComposerUsageLimits";
import { lazyParts } from "./lazyParts";

const UsageLimits = lazyParts<typeof LimitsChunk>(
  () => import("./ComposerUsageLimits"),
);

type UsagePayload = ChatUsage;
type UsageStyle = CSSProperties & { "--composer-context-usage": string };

function compactTokenCount(tokens: number): string {
  if (tokens >= 1_000 && tokens % 1_000 === 0) {
    return `${tokens / 1_000}K`;
  }
  return tokens.toLocaleString();
}

export default function UsageContext({
  usage,
  usageRequest = null,
}: {
  usage: UsagePayload;
  usageRequest?: AcpUsageRequest | null;
}) {
  const [limitsOpen, setLimitsOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const limitsId = useId();
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
    <p className="composer__context" aria-label="Composer context">
      <span className="composer__context-item composer__context-item--usage">
        <span className="composer__context-usage-label">
          {compactTokenCount(total)} context
        </span>
        {usageRequest ? (
          <button
            ref={triggerRef}
            type="button"
            className="composer__context-usage"
            title={`${tooltip} · Usage`}
            aria-label={`${formattedTotal} / ${formattedCapacity} context tokens used. Show usage`}
            aria-haspopup="dialog"
            aria-expanded={limitsOpen}
            aria-controls={limitsOpen ? limitsId : undefined}
            onClick={() => setLimitsOpen((open) => !open)}
            style={{ "--composer-context-usage": fill } as UsageStyle}
          />
        ) : (
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
        )}
      </span>
      {limitsOpen && usageRequest ? (
        <UsageLimits
          part="default"
          id={limitsId}
          request={usageRequest}
          triggerRef={triggerRef}
          onDismiss={() => setLimitsOpen(false)}
        />
      ) : null}
    </p>
  );
}
