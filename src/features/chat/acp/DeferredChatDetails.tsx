import type { ComponentProps } from "react";
import type { InlineSavedDiff } from "./InlineSavedDiff";
import type { TurnFileActivity } from "./TurnFileActivity";
import { LazyModuleBoundary } from "../../../ui/LazyModuleBoundary";

const loadInlineSavedDiff = () =>
  import("./InlineSavedDiff").then(({ InlineSavedDiff }) => ({
    default: InlineSavedDiff,
  }));

const loadTurnFiles = () =>
  import("./TurnFileActivity").then(({ TurnFileActivity }) => ({
    default: TurnFileActivity,
  }));
export function DeferredTurnFileActivity(
  props: ComponentProps<typeof TurnFileActivity>,
) {
  if (
    !props.turn.items.some((item) => item.kind === "fileChangeDiff") &&
    !props.workspaceChanges?.some(
      (change) => change.turnId === props.turn.id,
    ) &&
    !props.workspaceIncomplete
  )
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
