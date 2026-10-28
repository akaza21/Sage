import type { ComponentProps } from "react";
import type { InlineSavedDiff } from "./InlineSavedDiff";
import type { TurnFileActivity } from "./TurnFileActivity";
import type { TurnEditSummary } from "./TurnEditSummary";
import { LazyModuleBoundary } from "../../../ui/LazyModuleBoundary";

const loadInlineSavedDiff = () =>
  import("./InlineSavedDiff").then(({ InlineSavedDiff }) => ({
    default: InlineSavedDiff,
  }));

const loadTurnFiles = () =>
  import("./TurnFileActivity").then(({ TurnFileActivity }) => ({
    default: TurnFileActivity,
  }));
const loadEditSummary = () =>
  import("./TurnEditSummary").then(({ TurnEditSummary }) => ({
    default: TurnEditSummary,
  }));

export function DeferredTurnEditSummary(
  props: ComponentProps<typeof TurnEditSummary>,
) {
  return (
    <LazyModuleBoundary
      label="Changed files"
      load={loadEditSummary}
      fallback={null}
      render={(Surface) => <Surface {...props} />}
    />
  );
}

export function DeferredTurnFileActivity(
  props: ComponentProps<typeof TurnFileActivity>,
) {
  if (!props.turn.items.some((item) => item.kind === "fileChangeDiff"))
    return null;
  return (
    <LazyModuleBoundary
      label="Edited files"
      load={loadTurnFiles}
      render={(Surface) => <Surface {...props} />}
    />
  );
}

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
