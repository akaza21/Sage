import { lazy, Suspense } from "react";
import type { ChatUsage } from "./acp/chatUsage";
import "../../styles/composer-context.css";

const UsageContext = lazy(() => import("./ComposerUsageContext"));

export function ComposerContext({ usage }: { usage: ChatUsage | null }) {
  if (
    !usage ||
    !Number.isSafeInteger(usage.contextWindow) ||
    !usage.contextWindow ||
    usage.contextWindow <= 0
  )
    return null;
  return (
    <Suspense fallback={null}>
      <UsageContext usage={usage} />
    </Suspense>
  );
}
