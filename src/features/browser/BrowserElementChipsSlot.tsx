import { lazy, Suspense, type ComponentProps } from "react";
import type { AttachedContext } from "../../domain/attachedContext";

const BrowserElementChips = lazy(() => import("./BrowserElementChips"));

export function BrowserElementChipsSlot(
  props: ComponentProps<typeof BrowserElementChips>,
) {
  return (
    <Suspense fallback={null}>
      <BrowserElementChips {...props} />
    </Suspense>
  );
}

export function SentBrowserContext({
  contexts,
}: {
  contexts: readonly AttachedContext[];
}) {
  return contexts.some(({ kind }) => kind.startsWith("browser")) ? (
    <BrowserElementChipsSlot contexts={contexts} />
  ) : null;
}
