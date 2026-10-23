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
    const [active, setActive] = useState<"model" | "effort" | null>(null);
    const modelTrigger = useRef<HTMLButtonElement>(null);
    return (
      <form className="composer">
        <textarea aria-label="Message agent" />
        <ComposerModelControls
          triggerRef={modelTrigger}
          panelId="models"
          open={active === "model"}
          effortOpen={active === "effort"}
          onToggle={() => setActive("model")}
          onToggleEffort={() =>
            setActive(active === "effort" ? null : "effort")
          }
          onDismiss={() => setActive(null)}
          choices={[{ id: "actual", name: "Actual model" }]}
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

describe("Advertised composer effort", () => {
  it("distinguishes model and thinking controls when only their icons fit", () => {
    controls(effort("effort", ["low", "high"], "high"));
    const model = screen.getByRole("button", { name: "Model" });
    const thinking = screen.getByRole("button", {
      name: "Thinking level: high",
    });
    expect(model.querySelector(".lucide-sparkles")).not.toBeNull();
    expect(thinking.querySelector(".lucide-brain")).not.toBeNull();
    expect(thinking.querySelector(".lucide-sparkles")).toBeNull();
  });

  it.each(["medium", "Balanced thinking"])(
    "uses the %s display label in its heading and retains an advertised reset",
    (label) => {
      const { onSetOption } = controls({
        ...effort("effort", ["low", "medium"], "medium"),
        defaultValue: "low",
        options: [
          { value: "low", name: "Low" },
          { value: "medium", name: label },
        ],
      });
      fireEvent.click(
        screen.getByRole("button", { description: "Select effort" }),
      );
      const dialog = screen.getByRole("dialog", { name: "Thinking level" });
      expect(dialog.querySelector(".composer-effort__level")).toHaveTextContent(
        label === "medium" ? "Medium" : label,
      );
      const reset = screen.getByRole("button", { name: "Reset effort" });
      expect(reset).toBeVisible();
      expect(reset).toHaveAttribute("title", "Reset effort");
      fireEvent.click(reset);
      expect(onSetOption).toHaveBeenCalledExactlyOnceWith("effort", {
        value: "low",
      });
    },
  );

  it.each([
    ["Codex", "effort", ["minimal", "low", "medium", "high", "xhigh"], "xhigh"],
    ["Claude", "effort", ["low", "medium", "high"], "high"],
    ["Pi", "pi.thinking", ["off", "high"], "high"],
  ])(
    "previews a %s drag locally and commits only its final advertised value",
    (_name, id, values, final) => {
      const { onSetOption } = controls(effort(id, values));
      fireEvent.click(
        screen.getByRole("button", { description: "Select effort" }),
      );
      const slider = screen.getByRole("slider");
      fireEvent.pointerDown(slider, { pointerId: 1 });
      for (let position = 1; position < values.length; position++) {
        fireEvent.change(slider, { target: { value: String(position) } });
      }
      expect(onSetOption).not.toHaveBeenCalled();
      expect(slider).toHaveValue(String(values.length - 1));
      expect(slider).toHaveAttribute("aria-valuetext", final);
      fireEvent.pointerUp(slider, { pointerId: 1 });
      fireEvent.blur(slider);
      expect(onSetOption).toHaveBeenCalledExactlyOnceWith(id, {
        value: final,
      });
      expect(slider).toHaveValue("0");
    },
  );
  it("discards a cancelled or stale drag rather than mutating a replacement option", () => {
    const option = effort("effort", ["low", "medium", "high"]);
    const { rerender, Host, onSetOption } = controls(option);
    fireEvent.click(
      screen.getByRole("button", { description: "Select effort" }),
    );
    const slider = screen.getByRole("slider");
    fireEvent.pointerDown(slider);
    fireEvent.change(slider, { target: { value: "2" } });
    fireEvent.pointerCancel(slider);
    expect(slider).toHaveValue("0");
    fireEvent.pointerDown(slider);
    fireEvent.change(slider, { target: { value: "2" } });
    rerender(<Host effortOption={effort("pi.thinking", ["off", "high"])} />);
    fireEvent.pointerUp(slider);
    expect(slider).toHaveAttribute("aria-valuetext", "off");
    expect(onSetOption).not.toHaveBeenCalled();
  });
  it("places the model link and reset before the slider in DOM focus order", () => {
    controls({
      ...effort("effort", ["low", "high"], "high"),
      defaultValue: "low",
    });
    fireEvent.click(
      screen.getByRole("button", { description: "Select effort" }),
    );
    const dialog = screen.getByRole("dialog", { name: "Thinking level" });
    expect(
      Array.from(dialog.querySelectorAll("button, input")).map((control) =>
        control.getAttribute("aria-label"),
      ),
    ).toEqual(["Select model: Actual model", "Reset effort", "Thinking level"]);
  });
  it("shows the current level on the trigger with Select effort as its description", () => {
    const { rerender, Host } = controls(effort("effort", ["low", "high"]));
    const trigger = screen.getByRole("button", { name: "Thinking level: low" });
    expect(trigger).toHaveTextContent("low");
    expect(trigger).toHaveAccessibleDescription("Select effort");
    rerender(<Host effortOption={effort("effort", ["low", "high"], "high")} />);
    expect(trigger).toHaveAccessibleName("Thinking level: high");
    expect(trigger).toHaveTextContent("high");
  });
  it("restores the surviving effort trigger when closing from the model link", async () => {
    controls(effort("effort", ["low", "high"]));
    const trigger = screen.getByRole("button", {
      description: "Select effort",
    });
    fireEvent.click(trigger);
    await act(async () => {});
    screen.getByRole("button", { name: "Select model: Actual model" }).focus();
    fireEvent.click(trigger);
    await act(async () => {});
    expect(trigger).toHaveFocus();
  });
  it.each(["Escape", "Shift+Tab"])(
    "restores trigger focus on %s while a mutation is pending",
    async (key) => {
      const { rerender, Host } = controls(effort("effort", ["low", "high"]));
      const trigger = screen.getByRole("button", {
        description: "Select effort",
      });
      fireEvent.click(trigger);
      await act(async () => {});
      rerender(<Host pending />);
      const focused = screen.getByRole("slider");
      focused.focus();
      fireEvent.keyDown(focused, {
        key: key === "Shift+Tab" ? "Tab" : key,
        shiftKey: key === "Shift+Tab",
      });
      expect(screen.queryByRole("slider")).toBeNull();
      expect(trigger).toHaveFocus();
    },
  );
  it("dismisses effort when the composer becomes unavailable without reopening afterward", async () => {
    const { rerender, Host } = controls(effort("effort", ["low", "high"]));
    fireEvent.click(
      screen.getByRole("button", { description: "Select effort" }),
    );
    rerender(<Host disabled />);
    await act(async () => {});
    expect(screen.queryByRole("slider")).toBeNull();
    expect(
      screen.getByRole("textbox", { name: "Message agent" }),
    ).toHaveFocus();
    rerender(<Host />);
    expect(
      screen.getByRole("button", { description: "Select effort" }),
    ).toHaveAttribute("aria-expanded", "false");
  });
  it("returns withdrawn effort focus to the surviving model control", async () => {
    const { rerender, Host } = controls(effort("effort", ["low", "high"]));
    fireEvent.click(
      screen.getByRole("button", { description: "Select effort" }),
    );
    await act(async () => {});
    rerender(<Host effortOption={null} />);
    await act(async () => {});
    expect(screen.queryByRole("slider")).toBeNull();
    expect(screen.getByRole("button", { name: "Model" })).toHaveFocus();
  });

  it("returns reset focus to the slider before pending and default disable the reset", async () => {
    const option = {
      ...effort("effort", ["low", "high"], "high"),
      defaultValue: "low",
    };
    const { rerender, Host } = controls(option);
    fireEvent.click(
      screen.getByRole("button", { description: "Select effort" }),
    );
    await act(async () => {});
    const reset = screen.getByRole("button", { name: "Reset effort" });
    reset.focus();
    fireEvent.click(reset);
    rerender(<Host pending />);
    expect(screen.getByRole("slider")).toHaveFocus();
    rerender(<Host effortOption={{ ...option, currentValue: "low" }} />);
    expect(screen.getByRole("button", { name: "Reset effort" })).toBeDisabled();
    expect(screen.getByRole("slider")).toHaveFocus();
  });
  it.each([
    ["Codex", "effort", ["minimal", "low", "medium", "high", "xhigh"]],
    ["Claude", "effort", ["low", "medium", "high"]],
    ["Pi", "pi.thinking", ["off", "high"]],
  ])(
    "emits only exact %s advertised values and no fabricated stops",
    (_, id, values) => {
      const { onSetOption } = controls(effort(id, values));
      fireEvent.click(
        screen.getByRole("button", { description: "Select effort" }),
      );
      const slider = screen.getByRole("slider", { name: "Thinking level" });
      expect(slider).toHaveAttribute("min", "0");
      expect(slider).toHaveAttribute("max", String(values.length - 1));
      expect(slider).toHaveAttribute("aria-valuetext", values[0]);
      for (let index = 1; index < values.length; index++) {
        fireEvent.change(slider, { target: { value: String(index) } });
      }
      expect(onSetOption.mock.calls).toEqual(
        values.slice(1).map((value) => [id, { value }]),
      );
      expect(screen.queryByRole("combobox")).toBeNull();
      expect(screen.queryByRole("button", { name: "Reset effort" })).toBeNull();
    },
  );

  it("focuses the slider and steps advertised positions with arrows, Home and End", async () => {
    const option = effort("pi.thinking", ["off", "high", "maximum"], "high");
    const { onSetOption } = controls(option);
    const trigger = screen.getByRole("button", {
      description: "Select effort",
    });
    fireEvent.click(trigger);
    await act(async () => {});
    const slider = screen.getByRole("slider");
    expect(slider).toHaveFocus();
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
    expect(trigger).toHaveFocus();
    expect(screen.queryByRole("slider")).toBeNull();
  });
  it("steps from each newly confirmed level rather than an optimistic or stale index", () => {
    const option = effort("pi.thinking", ["off", "medium", "high"]);
    const { rerender, Host, onSetOption } = controls(option);
    fireEvent.click(
      screen.getByRole("button", { description: "Select effort" }),
    );
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

  it("resets only to a confirmed advertised default, never the first or current value", () => {
    const option = {
      ...effort("effort", ["low", "medium", "high"], "high"),
      defaultValue: "medium",
    };
    const { onSetOption, rerender, Host } = controls(option);
    fireEvent.click(
      screen.getByRole("button", { description: "Select effort" }),
    );
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

  it("keeps authoritative level and focus while pending, ignoring late mutation results", async () => {
    let resolve!: (value: unknown) => void;
    const onSetOption = vi.fn(
      () =>
        new Promise((accept) => {
          resolve = accept;
        }),
    );
    const option = effort("effort", ["low", "high"], "high");
    const { rerender, Host } = controls(option, onSetOption);
    fireEvent.click(
      screen.getByRole("button", { description: "Select effort" }),
    );
    await act(async () => {});
    const slider = screen.getByRole("slider");
    fireEvent.keyDown(slider, { key: "Home" });
    rerender(<Host pending />);
    expect(slider).toHaveFocus();
    expect(slider).toHaveAttribute("aria-valuetext", "high");
    fireEvent.keyDown(slider, { key: "Home" });
    expect(onSetOption).toHaveBeenCalledTimes(1);
    const replacement = effort("pi.thinking", ["off", "high"], "off");
    rerender(<Host effortOption={replacement} />);
    await act(async () => resolve({ currentValue: "low" }));
    expect(screen.getByRole("slider")).toHaveAttribute("aria-valuetext", "off");
  });

  it("opens the plain model list from the model link and keeps menus exclusive", async () => {
    controls(effort("effort", ["low", "high"]));
    fireEvent.click(
      screen.getByRole("button", { description: "Select effort" }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Select model: Actual model" }),
    );
    await act(async () => {});
    expect(screen.queryByRole("slider")).toBeNull();
    expect(screen.getByRole("option", { name: "Actual model" })).toHaveFocus();
  });

  it("omits an unadvertised effort control and cannot step outside a singleton catalog", () => {
    const { rerender, Host, onSetOption } = controls(undefined);
    expect(
      screen.queryByRole("button", { description: "Select effort" }),
    ).toBeNull();
    rerender(<Host effortOption={effort("effort", ["high"])} />);
    fireEvent.click(
      screen.getByRole("button", { description: "Select effort" }),
    );
    const slider = screen.getByRole("slider");
    for (const key of ["Home", "End", "ArrowLeft", "ArrowRight"])
      fireEvent.keyDown(slider, { key });
    expect(onSetOption).not.toHaveBeenCalled();
  });
});
