import { lazyParts } from "./lazyParts";
import type * as UsageChunk from "./ComposerUsageContext";
import type { AcpUsageRequest } from "../../domain/acpUsage";
import type { ChatUsage } from "./acp/chatUsage";
import "../../styles/composer-context.css";

const UsageContext = lazyParts<typeof UsageChunk>(
  () => import("./ComposerUsageContext"),
);

export function ComposerContext({
  usage,
  usageRequest = null,
  visible = true,
}: {
  usage: ChatUsage | null;
  usageRequest?: AcpUsageRequest | null;
  visible?: boolean;
}) {
  if (
    !usage ||
    !Number.isSafeInteger(usage.contextWindow) ||
    usage.contextWindow! <= 0
  )
    return null;
  return (
    <div className="composer__context-slot">
      {visible && (
        <UsageContext
          part="default"
          usage={usage}
          usageRequest={usageRequest}
        />
      )}
    </div>
  );
}
