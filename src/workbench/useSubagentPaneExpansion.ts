import { useEffect, useRef } from "react";
import type { SubagentPaneTarget } from "../app/subagentPaneStore";

export function useSubagentPaneExpansion({
  expanded,
  selected,
  target,
  onExpandedChange,
}: {
  expanded: boolean;
  selected: boolean;
  target: SubagentPaneTarget | null;
  onExpandedChange?: (expanded: boolean) => void;
}) {
  const previousSelection = useRef<{
    target: SubagentPaneTarget | null;
    selected: boolean;
  }>({ target: null, selected: false });
  useEffect(() => {
    const previous = previousSelection.current;
    previousSelection.current = { target, selected };
    if (
      selected &&
      !expanded &&
      (!previous.selected || previous.target !== target)
    )
      onExpandedChange?.(true);
  }, [expanded, onExpandedChange, selected, target]);
}
