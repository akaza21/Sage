import { cleanup, render, screen } from "@testing-library/react";
import { createRef } from "react";
import { afterEach, expect, it, vi } from "vitest";
import { ComposerModelControls } from "./ComposerModelControls";

const menu = vi.hoisted(() => ({
  handlers: [] as ((modelId: string) => void)[],
}));
vi.mock("./ComposerModelMenu", () => ({
  default: ({ onSelect }: { onSelect: (modelId: string) => void }) => {
    menu.handlers.push(onSelect);
    return <p>Model menu</p>;
  },
}));
afterEach(cleanup);

it("hands the model menu one select handler that calls the caller's latest callback", async () => {
  const triggerRef = createRef<HTMLButtonElement>();
  const controls = (onSelectModel: (modelId: string) => void) => (
    <ComposerModelControls
      triggerRef={triggerRef}
      panelId="models"
      open
      effortOpen={false}
      onToggle={() => {}}
      onToggleEffort={() => {}}
      onDismiss={() => {}}
      choices={[
        { id: "actual", name: "Actual model" },
        { id: "other", name: "Other model" },
      ]}
      selectedId="actual"
      modelLabel="Actual model"
      effortLabel={null}
      contextWindow={null}
      effortOption={undefined}
      disabled={false}
      pending={false}
      onSelectModel={onSelectModel}
      onSetOption={async () => {}}
    />
  );
  const first = vi.fn();
  const latest = vi.fn();
  const view = render(controls(first));
  await screen.findByText("Model menu");
  view.rerender(controls(latest));
  expect(menu.handlers.length).toBeGreaterThan(1);
  expect(new Set(menu.handlers).size).toBe(1);
  menu.handlers[0]!("other");
  expect(first).not.toHaveBeenCalled();
  expect(latest).toHaveBeenCalledExactlyOnceWith("other");
});
