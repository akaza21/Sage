import { lazy, Suspense, type ComponentProps } from "react";

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
