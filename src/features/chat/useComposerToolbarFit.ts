import { useCallback, useMemo, useRef } from "react";
import { useToolbarOverflow } from "../../ui/useToolbarOverflow";

// Lowest priority first; chat-container-controls.css keys on these data-fit tokens.
const TOOLBAR_STEPS = [
  "placeholder",
  "context",
  "capacity",
  "project",
  "effort",
  "access",
  "harness",
  "project-menu",
  "effort-menu",
  "model-shrink",
  "model",
  "dense",
  "wrap",
] as const;

export function useComposerToolbarFit(
  canSelectEffort: boolean,
  hasProject: boolean,
  paused: boolean,
) {
  const controls = useRef<HTMLDivElement>(null);
  const steps = useMemo(
    () =>
      TOOLBAR_STEPS.filter(
        (step) =>
          (step !== "effort-menu" || canSelectEffort) &&
          (step !== "project-menu" || hasProject),
      ),
    [canSelectEffort, hasProject],
  );
  const widths = useRef({ project: [0, 0], effort: [0, 0] });
  const tier = useToolbarOverflow(
    useCallback(() => {
      const row = controls.current;
      if (!row || paused) return;
      const optionsTrigger = () =>
        row.querySelector<HTMLButtonElement>(".composer__plus");
      return {
        row,
        steps: steps.length,
        search: "binary" as const,
        focus: {
          trigger: optionsTrigger,
          items: () => {
            const menuId = optionsTrigger()?.getAttribute("aria-controls");
            const menu = menuId ? document.getElementById(menuId) : null;
            return [row, ...(menu ? [menu] : [])].flatMap((root) => [
              ...root.querySelectorAll<HTMLElement>(
                "[data-overflow-control] button, button[data-overflow-control]",
              ),
            ]);
          },
        },
        measure: (taken: number) => {
          row.dataset.fit = steps.slice(0, taken).join(" ");
          for (const slot of row.querySelectorAll<HTMLElement>(
            "[data-overflow-control]",
          )) {
            const name = slot.dataset
              .overflowControl as keyof typeof widths.current;
            const cached = widths.current[name];
            const compacted = steps.indexOf(name) < taken ? 1 : 0;
            if (slot.hasAttribute("data-suspended"))
              slot.style.width = `${cached[compacted]}px`;
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
    }, [steps, paused]),
  );
  const taken = steps.slice(0, Math.max(0, tier));
  return {
    controls,
    effortInMenu: taken.includes("effort-menu"),
    projectInMenu: taken.includes("project-menu"),
    contextVisible: tier >= 0 && !taken.includes("context"),
  };
}
