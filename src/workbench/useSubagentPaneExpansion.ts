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
  const previousTarget = useRef<SubagentPaneTarget | null>(null);
  useEffect(() => {
    const previous = previousTarget.current;
    previousTarget.current = target;
    if (selected && !expanded && previous !== target) onExpandedChange?.(true);
  }, [expanded, onExpandedChange, selected, target]);
}
