import type { ComponentProps } from "react";
import type { InlineSavedDiff } from "./InlineSavedDiff";
import { LazyModuleBoundary } from "../../../ui/LazyModuleBoundary";

const loadInlineSavedDiff = () =>
  import("./InlineSavedDiff").then(({ InlineSavedDiff }) => ({
    default: InlineSavedDiff,
  }));

export function DeferredInlineSavedDiff(
  props: ComponentProps<typeof InlineSavedDiff>,
) {
  return (
    <LazyModuleBoundary
      label="Saved diff"
      load={loadInlineSavedDiff}
      render={(Surface) => <Surface {...props} />}
    />
  );
}
