import { useCallback, useRef } from "react";
import { useToolbarOverflow } from "../../ui/useToolbarOverflow";

/**
 * The toolbar's compaction steps, lowest priority first, as data-fit tokens
 * that chat-container-controls.css keys on. A disabled placeholder goes first
 * and the model name last: it truncates to a readable prefix before it gives
 * way to its icon.
 */
const TOOLBAR_STEPS = [
  "placeholder",
  "capacity",
  "access",
  "harness",
  "project",
  "effort",
  "effort-menu",
  "project-menu",
  "model-shrink",
  "model",
  "dense",
  "wrap",
] as const;

function composerSteps(canSelectEffort: boolean, hasProject: boolean) {
  return TOOLBAR_STEPS.filter(
    (step) =>
      (step !== "effort-menu" || canSelectEffort) &&
      (step !== "project-menu" || hasProject),
  );
}

/**
 * Fits the composer toolbar's controls row. Effort and the project chip move
 * into the options menu only when there is a menu item to take their place;
 * while a popover from the row is open, the row keeps its steps.
 */
export function useComposerToolbarFit(
  canSelectEffort: boolean,
  hasProject: boolean,
  paused: boolean,
) {
  const controls = useRef<HTMLDivElement>(null);
  const steps = composerSteps(canSelectEffort, hasProject);
  const widths = useRef({ project: [0, 0], effort: [0, 0] });
  const tier = useToolbarOverflow(
    useCallback(() => {
      const row = controls.current;
      if (!row || paused) return;
      const names = composerSteps(canSelectEffort, hasProject);
      return {
        row,
        steps: names.length,
        search: "binary" as const,
        measure: (taken: number) => {
          row.setAttribute("data-fit", names.slice(0, taken).join(" "));
          for (const slot of row.querySelectorAll<HTMLElement>(
            "[data-overflow-control]",
          )) {
            const name = slot.dataset
              .overflowControl as keyof typeof widths.current;
            const cached = widths.current[name];
            const compacted = names.indexOf(name) < taken ? 1 : 0;
            if (slot.hasAttribute("data-suspended"))
              slot.style.width = `${cached[compacted] ?? 0}px`;
            else {
              const width = slot.getBoundingClientRect().width;
              if (width > 0) cached[compacted] = width;
            }
          }
          const edge = row.getBoundingClientRect().right + 0.5;
          return [...row.children].some(
            (child) => child.getBoundingClientRect().right > edge,
          );
        },
      };
    }, [canSelectEffort, hasProject, paused]),
  );
  const taken = steps.slice(0, tier);
  return {
    controls,
    effortInMenu: taken.includes("effort-menu"),
    projectInMenu: taken.includes("project-menu"),
  };
}
