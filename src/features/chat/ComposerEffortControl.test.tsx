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

describe("Composer model and thinking level", () => {
  it("shows the level on the model chip and changes it inside the model popover", async () => {
    const { rerender, Host } = controls(effort("effort", ["low", "high"]));
    const model = screen.getByRole("button", { name: "Model" });
    expect(model).toHaveTextContent("Actual modelLow");
    expect(screen.queryByRole("button", { name: /Thinking level/ })).toBeNull();
    const dialog = await openPopover();
    expect(dialog).toContainElement(screen.getByRole("listbox"));
    expect(
      screen.getByRole("group", { name: "Thinking level" }),
    ).toContainElement(screen.getByRole("slider", { name: "Thinking level" }));
    rerender(<Host effortOption={effort("effort", ["low", "high"], "high")} />);
    expect(model).toHaveTextContent("Actual modelHigh");
  });

  it("starts on the selected model, then tabs to the reset and the slider", async () => {
    controls({
      ...effort("effort", ["low", "high"], "high"),
      defaultValue: "low",
    });
    const dialog = await openPopover();
    expect(screen.getByRole("option", { name: "Actual model" })).toHaveFocus();
    expect(
      Array.from(dialog.querySelectorAll("[tabindex='0'], input, button"))
        .filter((control) => control.getAttribute("tabindex") !== "-1")
        .map(
          (control) =>
            control.getAttribute("aria-label") ?? control.textContent?.trim(),
        ),
    ).toEqual(["Actual model", "Reset effort", "Thinking level"]);
  });

  it.each(["medium", "Balanced thinking"])(
    "uses the %s display label in its heading and retains an advertised reset",
    async (label) => {
      const { onSetOption } = controls({
        ...effort("effort", ["low", "medium"], "medium"),
        defaultValue: "low",
        options: [
          { value: "low", name: "Low" },
          { value: "medium", name: label },
        ],
      });
      const shown = label === "medium" ? "Medium" : label;
      expect(screen.getByRole("button", { name: "Model" })).toHaveTextContent(
        shown,
      );
      const dialog = await openPopover();
      expect(dialog.querySelector(".composer-effort__level")).toHaveTextContent(
        shown,
      );
      const reset = screen.getByRole("button", { name: "Reset effort" });
      expect(reset).toHaveAttribute("title", "Reset effort");
      fireEvent.click(reset);
      expect(onSetOption).toHaveBeenCalledExactlyOnceWith("effort", {
        value: "low",
      });
      expect(screen.getByRole("slider")).toHaveFocus();
    },
  );

  it.each([
    ["Codex", "effort", ["minimal", "low", "medium", "high", "xhigh"], "xhigh"],
    ["Claude", "effort", ["low", "medium", "high"], "high"],
    ["Pi", "pi.thinking", ["off", "high"], "high"],
  ])(
    "previews a %s drag locally and commits only its final advertised value",
    async (_name, id, values, final) => {
      const { onSetOption } = controls(effort(id, values));
      const dialog = await openPopover();
      const slider = screen.getByRole("slider");
      expect(slider).toHaveAttribute("max", String(values.length - 1));
      expect(
        dialog.querySelectorAll(".composer-effort__stops span"),
      ).toHaveLength(values.length);
      const heading = dialog.querySelector(".composer-effort__level");
      const shown = (value: string) => value[0].toUpperCase() + value.slice(1);
      fireEvent.pointerDown(slider, { pointerId: 1 });
      for (let position = 1; position < values.length; position++) {
        fireEvent.change(slider, { target: { value: String(position) } });
        expect(heading).toHaveTextContent(shown(values[position]));
      }
      expect(onSetOption).not.toHaveBeenCalled();
      fireEvent.pointerUp(slider, { pointerId: 1 });
      fireEvent.blur(slider);
      expect(onSetOption).toHaveBeenCalledExactlyOnceWith(id, {
        value: final,
      });
      expect(slider).toHaveValue("0");
    },
  );

  it("discards a cancelled or stale drag rather than mutating a replacement option", async () => {
    const { rerender, Host, onSetOption } = controls(
      effort("effort", ["low", "medium", "high"]),
    );
    await openPopover();
    const slider = screen.getByRole("slider");
    fireEvent.pointerDown(slider);
    fireEvent.change(slider, { target: { value: "2" } });
    fireEvent.pointerCancel(slider);
    expect(slider).toHaveValue("0");
    fireEvent.pointerDown(slider);
    fireEvent.change(slider, { target: { value: "2" } });
    rerender(<Host effortOption={effort("pi.thinking", ["off", "high"])} />);
    fireEvent.pointerUp(slider);
    expect(slider).toHaveAttribute("aria-valuetext", "Off");
    expect(onSetOption).not.toHaveBeenCalled();
  });

  it("steps advertised positions with arrows, Home and End, and Escape returns to the chip", async () => {
    const { onSetOption } = controls(
      effort("pi.thinking", ["off", "high", "maximum"], "high"),
    );
    await openPopover();
    const slider = screen.getByRole("slider");
    slider.focus();
    for (const key of [
      "ArrowLeft",
      "ArrowDown",
      "ArrowRight",
      "ArrowUp",
      "Home",
      "End",
    ])
      fireEvent.keyDown(slider, { key });
    expect(onSetOption.mock.calls).toEqual([
      ["pi.thinking", { value: "off" }],
      ["pi.thinking", { value: "off" }],
      ["pi.thinking", { value: "maximum" }],
      ["pi.thinking", { value: "maximum" }],
      ["pi.thinking", { value: "off" }],
      ["pi.thinking", { value: "maximum" }],
    ]);
    fireEvent.keyDown(slider, { key: "Escape" });
    expect(screen.queryByRole("slider")).toBeNull();
    expect(screen.getByRole("button", { name: "Model" })).toHaveFocus();
  });

  it("keeps the popover open and the slider focused while a level change is pending", async () => {
    const { rerender, Host, onSetOption } = controls(
      effort("effort", ["low", "high"], "high"),
    );
    await openPopover();
    const slider = screen.getByRole("slider");
    slider.focus();
    fireEvent.keyDown(slider, { key: "Home" });
    rerender(<Host pending />);
    await act(async () => {});
    expect(screen.getByRole("dialog", { name: "Select model" })).toBeVisible();
    expect(slider).toHaveFocus();
    expect(slider).toHaveAttribute("aria-valuetext", "High");
    fireEvent.keyDown(slider, { key: "Home" });
    expect(onSetOption).toHaveBeenCalledTimes(1);
  });

  it("steps from each newly confirmed level rather than an optimistic or stale index", async () => {
    const option = effort("pi.thinking", ["off", "medium", "high"]);
    const { rerender, Host, onSetOption } = controls(option);
    await openPopover();
    fireEvent.keyDown(screen.getByRole("slider"), { key: "ArrowRight" });
    rerender(<Host effortOption={{ ...option, currentValue: "medium" }} />);
    fireEvent.keyDown(screen.getByRole("slider"), { key: "ArrowRight" });
    rerender(<Host effortOption={{ ...option, currentValue: "high" }} />);
    fireEvent.keyDown(screen.getByRole("slider"), { key: "End" });
    fireEvent.keyDown(screen.getByRole("slider"), { key: "ArrowLeft" });
    expect(onSetOption.mock.calls).toEqual([
      ["pi.thinking", { value: "medium" }],
      ["pi.thinking", { value: "high" }],
      ["pi.thinking", { value: "medium" }],
    ]);
  });

  it("resets only to a confirmed advertised default", async () => {
    const option = {
      ...effort("effort", ["low", "medium", "high"], "high"),
      defaultValue: "medium",
    };
    const { onSetOption, rerender, Host } = controls(option);
    await openPopover();
    fireEvent.click(screen.getByRole("button", { name: "Reset effort" }));
    expect(onSetOption).toHaveBeenCalledExactlyOnceWith("effort", {
      value: "medium",
    });
    rerender(<Host effortOption={{ ...option, currentValue: "medium" }} />);
    expect(screen.getByRole("button", { name: "Reset effort" })).toBeDisabled();
    rerender(
      <Host effortOption={{ ...option, defaultValue: "unsupported" }} />,
    );
    expect(screen.queryByRole("button", { name: "Reset effort" })).toBeNull();
  });

  it("closes when the composer becomes unavailable", async () => {
    const { rerender, Host } = controls(effort("effort", ["low", "high"]));
    await openPopover();
    rerender(<Host disabled />);
    await act(async () => {});
    expect(screen.queryByRole("slider")).toBeNull();
    rerender(<Host />);
    expect(screen.getByRole("button", { name: "Model" })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
  });

  it("lists models without a thinking section when the harness advertises none", async () => {
    controls(undefined);
    expect(screen.getByRole("button", { name: "Model" })).toHaveTextContent(
      "Actual modelHigh",
    );
    await openPopover();
    expect(screen.getAllByRole("option")).toHaveLength(2);
    expect(screen.queryByRole("slider")).toBeNull();
  });
});
