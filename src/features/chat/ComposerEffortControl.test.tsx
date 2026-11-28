import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useRef, useState } from "react";
import type { AcpConfigOption } from "../../domain/acpSessionConfig";
import { ComposerModelControls } from "./ComposerModelControls";

afterEach(cleanup);

function controls(
  option: AcpConfigOption | undefined,
  onSetOption = vi.fn().mockResolvedValue(undefined),
) {
  function Host({
    effortOption = option,
    pending = false,
    disabled = false,
  }: {
    effortOption?: AcpConfigOption | null;
    pending?: boolean;
    disabled?: boolean;
  }) {
    const [open, setOpen] = useState(false);
    const modelTrigger = useRef<HTMLButtonElement>(null);
    return (
      <form className="composer">
        <textarea aria-label="Message agent" />
        <ComposerModelControls
          triggerRef={modelTrigger}
          panelId="models"
          open={open}
          onToggle={() => setOpen(!open)}
          onDismiss={() => setOpen(false)}
          choices={[
            { id: "actual", name: "Actual model" },
            { id: "other", name: "Other model" },
          ]}
          selectedId="actual"
          modelLabel="Actual model"
          effortLabel="High"
          contextWindow={null}
          effortOption={effortOption ?? undefined}
          disabled={disabled}
          pending={pending}
          onSelectModel={() => {}}
          onSetOption={onSetOption}
        />
      </form>
    );
  }
  return { ...render(<Host />), Host, onSetOption };
}

function effort(
  id: string,
  values: string[],
  currentValue = values[0],
): Extract<AcpConfigOption, { type: "select" }> {
  return {
    id,
    name: "Thinking level",
    category: "effort",
    type: "select",
    currentValue,
    options: values.map((value) => ({ value, name: value })),
  };
}

async function openPopover() {
  fireEvent.click(screen.getByRole("button", { name: "Model" }));
  const dialog = await screen.findByRole("dialog", { name: "Select model" });
  await screen.findByRole("listbox");
  return dialog;
}

const trigger = () => screen.getByRole("button", { name: /Thinking effort/ });

describe("Composer model and thinking effort", () => {
  it("shows the level on the model chip and on the popover's Thinking effort row", async () => {
    const { rerender, Host } = controls(effort("effort", ["low", "high"]));
    const model = screen.getByRole("button", { name: "Model" });
    expect(model).toHaveTextContent("Actual modelLow");
    expect(screen.queryByRole("button", { name: /Thinking effort/ })).toBeNull();
    const dialog = await openPopover();
    expect(dialog).toContainElement(screen.getByRole("listbox"));
    expect(await screen.findByRole("button", { name: /Thinking effort/ })).toHaveTextContent(
      "Thinking effortLow",
    );
    rerender(<Host effortOption={effort("effort", ["low", "high"], "high")} />);
    expect(model).toHaveTextContent("Actual modelHigh");
    expect(trigger()).toHaveTextContent("Thinking effortHigh");
  });

  it("starts on the selected model, then tabs to the Thinking effort row", async () => {
    controls(effort("effort", ["low", "high"], "high"));
    const dialog = await openPopover();
    await screen.findByRole("button", { name: /Thinking effort/ });
    expect(screen.getByRole("option", { name: "Actual model" })).toHaveFocus();
    expect(
      Array.from(dialog.querySelectorAll("[tabindex='0'], button"))
        .filter((control) => control.getAttribute("tabindex") !== "-1")
        .map((control) => control.textContent?.trim()),
    ).toEqual(["Actual model", "Thinking effortHigh"]);
  });

  it.each([
    ["Codex", "effort", ["minimal", "low", "medium", "high", "xhigh"], "xhigh"],
    ["Claude", "effort", ["low", "medium", "high"], "high"],
    ["Pi", "pi.thinking", ["off", "high"], "high"],
  ])(
    "lists %s's advertised levels in a submenu and sends only the one picked",
    async (_name, id, values, picked) => {
      const { onSetOption } = controls({
        ...effort(id, values),
        defaultValue: values[0],
      });
      await openPopover();
      fireEvent.click(await screen.findByRole("button", { name: /Thinking effort/ }));
      const menu = await screen.findByRole("menu", { name: "Thinking effort" });
      const levels = screen.getAllByRole("menuitemradio");
      expect(levels.map((level) => level.textContent)).toEqual(
        values.map(
          (value, index) =>
            value[0].toUpperCase() + value.slice(1) + (index ? "" : "Default"),
        ),
      );
      expect(levels[0]).toHaveAttribute("aria-checked", "true");
      expect(menu).toBeVisible();
      fireEvent.click(levels[0]);
      expect(onSetOption).not.toHaveBeenCalled();
      expect(trigger()).toHaveFocus();
      fireEvent.click(trigger());
      const name = picked[0].toUpperCase() + picked.slice(1);
      fireEvent.click(await screen.findByRole("menuitemradio", { name }));
      expect(onSetOption).toHaveBeenCalledExactlyOnceWith(id, {
        value: picked,
      });
      expect(screen.queryByRole("menu", { name: "Thinking effort" })).toBeNull();
    },
  );

  it("opens the submenu with ArrowRight and closes it to its row with ArrowLeft", async () => {
    controls(effort("pi.thinking", ["off", "high"], "high"));
    await openPopover();
    const row = await screen.findByRole("button", { name: /Thinking effort/ });
    row.focus();
    fireEvent.keyDown(row, { key: "ArrowRight" });
    const current = await screen.findByRole("menuitemradio", { name: "High" });
    expect(row).toHaveAttribute("aria-expanded", "true");
    fireEvent.keyDown(current, { key: "ArrowLeft" });
    expect(screen.queryByRole("menu", { name: "Thinking effort" })).toBeNull();
    expect(row).toHaveFocus();
  });

  it("holds the level while a change is pending", async () => {
    const { rerender, Host, onSetOption } = controls(
      effort("effort", ["low", "high"], "high"),
    );
    await openPopover();
    await screen.findByRole("button", { name: /Thinking effort/ });
    rerender(<Host pending />);
    await act(async () => {});
    expect(screen.getByRole("dialog", { name: "Select model" })).toBeVisible();
    expect(trigger()).toHaveAttribute("aria-disabled", "true");
    fireEvent.click(trigger());
    expect(screen.queryByRole("menu", { name: "Thinking effort" })).toBeNull();
    expect(onSetOption).not.toHaveBeenCalled();
  });

  it("closes when the composer becomes unavailable", async () => {
    const { rerender, Host } = controls(effort("effort", ["low", "high"]));
    await openPopover();
    rerender(<Host disabled />);
    await act(async () => {});
    expect(screen.queryByRole("button", { name: /Thinking effort/ })).toBeNull();
    rerender(<Host />);
    expect(screen.getByRole("button", { name: "Model" })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
  });

  it("lists models without a thinking row when the harness advertises none", async () => {
    controls(undefined);
    expect(screen.getByRole("button", { name: "Model" })).toHaveTextContent(
      "Actual modelHigh",
    );
    await openPopover();
    expect(screen.getAllByRole("option")).toHaveLength(2);
    expect(screen.queryByRole("button", { name: /Thinking effort/ })).toBeNull();
  });

  it("describes the chip with the level it shows, not a stale caller label", () => {
    controls(effort("effort", ["low", "high"], "low"));
    expect(
      screen.getByRole("button", { name: "Model" }),
    ).toHaveAccessibleDescription("Actual model Low");
  });

  it("opens the level while the model list is still loading", async () => {
    const { rerender, Host } = controls(effort("effort", ["low", "high"]));
    rerender(<Host pending />);
    const model = screen.getByRole("button", { name: "Model" });
    expect(model).toBeEnabled();
    fireEvent.click(model);
    expect(
      await screen.findByRole("button", { name: /Thinking effort/ }),
    ).toHaveAttribute("aria-disabled", "true");
  });
});
