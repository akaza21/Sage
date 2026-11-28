import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { ComposerSectionRows as ComposerAddActions } from "./ComposerSectionMenu";

afterEach(cleanup);

it("shows advertised modes by their own names and hides no id as a default", () => {
  const onSetSessionMode = vi.fn().mockResolvedValue(undefined);
  render(
    <ComposerAddActions
      sessionConfig={{
        sageAgentSessionId: "session",
        runtimeEpoch: "1",
        configRevision: "1",
        modes: {
          currentModeId: "plan",
          availableModes: [
            { id: "default", name: "Manual" },
            { id: "plan", name: "Plan" },
            { id: "custom", name: "Default" },
          ],
        },
        configOptions: [],
      }}
      pending={false}
      disabled={false}
      onClose={() => {}}
      onSetSessionMode={onSetSessionMode}
      onSetSessionConfigOption={vi.fn()}
    />,
  );
  expect(
    screen
      .getAllByRole("menuitemradio")
      .map((item) => item.getAttribute("aria-label")),
  ).toEqual(["Manual", "Plan mode", "Default"]);
  const plan = screen.getByRole("menuitemradio", { name: "Plan mode" });
  expect(plan).toHaveAttribute("aria-checked", "true");
  fireEvent.click(plan);
  expect(onSetSessionMode).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("menuitemradio", { name: "Manual" }));
  expect(onSetSessionMode).toHaveBeenCalledExactlyOnceWith("default");
});
