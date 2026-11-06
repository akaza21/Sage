import { expect, type Locator } from "@playwright/test";

type SettleWindow = Window & {
  settleComposerRow?: (row: Element) => Promise<void>;
};

/**
 * Installs `window.settleComposerRow(row)`, which resolves once the row's
 * overflow fit has run for its current geometry.
 *
 * The fit refits on the frame after a ResizeObserver delivery, and under CPU
 * load a delivery can lag several frames behind layout, so a run of
 * unchanged frames does not prove the row is settled. This observes the row
 * and its controls as the fit does, and resolves on the first frame after
 * every observed element's current size has been delivered.
 */
export function installComposerRowSettle() {
  const page = window as SettleWindow;
  page.settleComposerRow ??= (row) =>
    new Promise((resolve) => {
      const delivered = new Map<Element, string>();
      const size = (width: number, height: number) => `${width}x${height}`;
      const targets = () => [row, ...row.children];
      let check = 0;
      const observer = new ResizeObserver((entries) => {
        for (const entry of entries) {
          const [box] = entry.borderBoxSize;
          delivered.set(entry.target, size(box.inlineSize, box.blockSize));
        }
        // The fit runs on the next frame; check after it.
        cancelAnimationFrame(check);
        check = requestAnimationFrame(() => {
          const pending = targets().filter((target) => {
            observer.observe(target);
            const box = target.getBoundingClientRect();
            return delivered.get(target) !== size(box.width, box.height);
          });
          if (pending.length > 0) return;
          observer.disconnect();
          resolve();
        });
      });
      for (const target of targets()) observer.observe(target);
    });
}

/**
 * The composer toolbar's compaction steps once its fit has run for the
 * current layout. An open popover holds the steps, so a test settles the row
 * before it reads where a control is or opens a menu.
 */
export async function settledComposerFit(composer: Locator) {
  await composer.page().evaluate(installComposerRowSettle);
  const fit = await composer
    .locator(".composer__controls")
    .evaluate(async (row) => {
      await (window as SettleWindow).settleComposerRow!(row);
      return row.getAttribute("data-fit") ?? "";
    });
  return fit.split(" ").filter(Boolean);
}

export async function expectCompactComposerTargets(composer: Locator) {
  const targets = await composer
    .locator(
      [
        ".composer__plus",
        ".composer__access-label",
        ".composer__harness button.searchable-picker__trigger",
        ".composer__model-trigger",
        ".composer__effort-picker",
        ".project-scope button.searchable-picker__trigger",
        ".composer__mic",
        ".send-button",
      ].join(","),
    )
    .evaluateAll((elements) =>
      elements
        .map((element) => {
          const { x, y, width, height } = element.getBoundingClientRect();
          return { x, y, width, height };
        })
        .filter((box) => box.width > 0 && box.height > 0),
    );
  expect(targets.length).toBeGreaterThanOrEqual(5);
  const ordered = targets.sort((left, right) => left.x - right.x);
  const bounds = (await composer.boundingBox())!;
  for (const [index, box] of ordered.entries()) {
    expect(box.width).toBeGreaterThanOrEqual(24);
    expect(box.height).toBeGreaterThanOrEqual(24);
    expect(box.x).toBeGreaterThanOrEqual(bounds.x);
    expect(box.x + box.width).toBeLessThanOrEqual(bounds.x + bounds.width);
    expect(
      Math.abs(box.y + box.height / 2 - (ordered[0].y + ordered[0].height / 2)),
    ).toBeLessThanOrEqual(1);
    if (index > 0)
      expect(box.x).toBeGreaterThanOrEqual(
        ordered[index - 1].x + ordered[index - 1].width,
      );
  }
}

/**
 * Toolbar labels from lowest to highest priority. A label may compact only
 * after every lower-priority label that is present has compacted. The
 * context meter beside the send action goes right after the disabled effort
 * placeholder.
 */
export const COMPOSER_LABEL_PRIORITY = [
  ".composer__effort-picker:not([aria-controls]) .composer__effort-label",
  ".composer__context-slot",
  ".composer__capacity",
  ".project-scope .searchable-picker__trigger-label",
  ".composer__effort-picker[data-overflow-control] .composer__effort-label",
  ".composer__access-text",
  ".composer__harness .searchable-picker__trigger-label",
  ".composer__model-label",
];

/**
 * Sets the composer's width to each of `widths` in turn, lets the toolbar fit
 * settle, and measures what the fit produced: which labels show, whether any
 * is clipped, whether the controls overflow the row, and whether the row
 * took a step (or dropped the model name or context meter) that it could
 * undo and still fit. `data-fit` only names the steps taken; the verdicts
 * come from layout. Restores the composer's own width afterwards.
 */
export async function sweepComposerToolbar(
  composer: Locator,
  widths: readonly number[],
) {
  await composer.page().evaluate(installComposerRowSettle);
  return composer.evaluate(
    async (element, { priority, widths }) => {
      const settle = (
        window as Window & {
          settleComposerRow?: (row: Element) => Promise<void>;
        }
      ).settleComposerRow!;
      const controls = element.querySelector<HTMLElement>(
        ".composer__controls",
      )!;
      const readableCharacter = (label: HTMLElement) => {
        const probe = document.createElement("span");
        probe.textContent = "0";
        probe.style.font = getComputedStyle(label).font;
        document.body.append(probe);
        const width = probe.getBoundingClientRect().width;
        probe.remove();
        return width;
      };
      const results = [];
      for (const width of widths) {
        element.style.width = `${width}px`;
        await settle(controls);
        const labels = priority.map((selector) => {
          const label = element.querySelector<HTMLElement>(selector);
          const box = label?.getBoundingClientRect();
          const shown = Boolean(box && box.width > 0);
          const host = label?.closest(
            "button, .composer__model-trigger, .composer__capacity, .composer__context-slot, label",
          );
          const hostBox = host?.getBoundingClientRect();
          return {
            present: Boolean(label),
            shown,
            // A label may shorten only to an ellipsis after a readable
            // prefix of eight characters; it is never cut off by its control.
            clipped:
              shown &&
              (box!.right > hostBox!.right + 0.5 ||
                box!.left < hostBox!.left - 0.5 ||
                (label!.scrollWidth > label!.clientWidth &&
                  (getComputedStyle(label!).textOverflow !== "ellipsis" ||
                    label!.clientWidth < 8 * readableCharacter(label!) - 0.5))),
          };
        });
        const edge =
          controls.getBoundingClientRect().right -
          Number.parseFloat(getComputedStyle(controls).paddingRight);
        const overflow = [...controls.children].some(
          (child) =>
            !["absolute", "fixed"].includes(getComputedStyle(child).position) &&
            child.getBoundingClientRect().right > edge + 0.5,
        );
        // The model name gives way to its icon only when the row cannot fit
        // it whole or truncated: undoing either step must overflow the row.
        const fit = controls.getAttribute("data-fit") ?? "";
        const steps = fit.split(" ");
        const fitsBefore = (step: string) => {
          controls.setAttribute(
            "data-fit",
            steps.slice(0, steps.indexOf(step)).join(" "),
          );
          const right = controls.getBoundingClientRect().right + 0.5;
          return ![...controls.children].some(
            (child) => child.getBoundingClientRect().right > right,
          );
        };
        const modelHiddenThoughItFits =
          steps.includes("model") &&
          (fitsBefore("model-shrink") || fitsBefore("model"));
        // The row takes no more steps than it needs: undoing the last one
        // must overflow it.
        const taken = steps.filter(Boolean);
        const overCompacted =
          taken.length > 0 && fitsBefore(taken[taken.length - 1]);
        controls.setAttribute("data-fit", fit);
        // With every control label shown, the meter is hidden only if
        // giving its column back would overflow the measured row.
        const slot = element.querySelector<HTMLElement>(
          ".composer__context-slot",
        );
        let meterHiddenThoughItFits = false;
        if (
          slot &&
          slot.getBoundingClientRect().width === 0 &&
          labels
            .slice(priority.indexOf(".composer__context-slot") + 1)
            .every((label) => !label.present || label.shown)
        ) {
          slot.style.display = "block";
          const right = controls.getBoundingClientRect().right + 0.5;
          meterHiddenThoughItFits = ![...controls.children].some(
            (child) => child.getBoundingClientRect().right > right,
          );
          slot.style.display = "";
        }
        results.push({
          width,
          labels,
          overflow,
          overCompacted,
          modelHiddenThoughItFits,
          meterHiddenThoughItFits,
        });
      }
      element.style.width = "";
      return results;
    },
    { priority: COMPOSER_LABEL_PRIORITY, widths },
  );
}

type ComposerSweep = Awaited<ReturnType<typeof sweepComposerToolbar>>;

/**
 * At every swept width the controls stay inside the row, no label is
 * clipped, the row takes no step it does not need, labels give way in
 * priority order, and a label never comes back while the composer narrows.
 */
export function expectComposerSweep(sweep: ComposerSweep) {
  let previous = sweep[0];
  for (const step of sweep) {
    const at = `at ${step.width}px`;
    expect(step.overflow, `controls overflow ${at}`).toBe(false);
    expect(step.overCompacted, `more steps than the row needs ${at}`).toBe(
      false,
    );
    expect(
      step.modelHiddenThoughItFits,
      `model name hidden though it fits ${at}`,
    ).toBe(false);
    expect(
      step.meterHiddenThoughItFits,
      `context meter hidden though it fits ${at}`,
    ).toBe(false);
    step.labels.forEach((label, index) => {
      expect(
        label.clipped,
        `${COMPOSER_LABEL_PRIORITY[index]} clipped ${at}`,
      ).toBe(false);
      if (!label.shown) return;
      for (const higher of step.labels.slice(index + 1))
        expect(
          !higher.present || higher.shown,
          `${COMPOSER_LABEL_PRIORITY[index]} outlasts a higher-priority label ${at}`,
        ).toBe(true);
      expect(
        previous.labels[index].shown || !previous.labels[index].present,
        `${COMPOSER_LABEL_PRIORITY[index]} reappears while narrowing ${at}`,
      ).toBe(true);
    });
    previous = step;
  }
}
