import { lazy, Suspense, type ComponentProps } from "react";
import type { AttachedContext } from "../../domain/attachedContext";

const BrowserElementChips = lazy(() => import("./BrowserElementChips"));

/** The composer's Annotate chips; their chunk loads with the first element. */
export function BrowserElementChipsSlot(
  props: ComponentProps<typeof BrowserElementChips>,
) {
  return (
    <Suspense fallback={null}>
      <BrowserElementChips {...props} />
    </Suspense>
  );
}

/** A sent turn's page elements and pages, read-only (contract §3, §4). */
export function SentBrowserContext({
  contexts,
}: {
  contexts: readonly AttachedContext[];
}) {
  // Its chunk loads only for a turn that carries a page or an element.
  return contexts.some(({ kind }) => kind.startsWith("browser")) ? (
    <BrowserElementChipsSlot contexts={contexts} />
  ) : null;
}
