import { lazyParts } from "./lazyParts";
import type * as UsageChunk from "./ComposerUsageContext";
import type { ChatUsage } from "./acp/chatUsage";
import "../../styles/composer-context.css";

const UsageContext = lazyParts<typeof UsageChunk>(
  () => import("./ComposerUsageContext"),
);

export function ComposerContext({
  usage,
  visible = true,
}: {
  usage: ChatUsage | null;
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
      {visible && <UsageContext part="default" usage={usage} />}
    </div>
  );
}
