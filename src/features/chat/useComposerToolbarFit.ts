import { useCallback, useRef } from "react";
import { useToolbarOverflow } from "../../ui/useToolbarOverflow";

// Lowest priority first; chat-container-controls.css keys on these data-fit tokens.
const TOOLBAR_STEPS = [
  "placeholder",
  "context",
  "effort",
  "model-shrink",
  "model",
  "dense",
  "wrap",
] as const;

export function useComposerToolbarFit(paused: boolean) {
  const controls = useRef<HTMLDivElement>(null);
  const tier = useToolbarOverflow(
    useCallback(() => {
      const row = controls.current;
      if (!row || paused) return;
      return {
        row,
        steps: TOOLBAR_STEPS.length,
        search: "binary" as const,
        // No control moves into the "+" menu, so focus never follows one.
        focus: {
          trigger: () =>
            row.querySelector<HTMLButtonElement>(".composer__plus"),
          items: () => [],
        },
        measure: (taken: number) => {
          row.dataset.fit = TOOLBAR_STEPS.slice(0, taken).join(" ");
          const edge = row.getBoundingClientRect().right + 0.5;
          return [...row.children].some(
            (child) => child.getBoundingClientRect().right > edge,
          );
        },
      };
    }, [paused]),
  );
  const taken = TOOLBAR_STEPS.slice(0, Math.max(0, tier));
  return {
    controls,
    contextVisible: tier >= 0 && !taken.includes("context"),
  };
}
