import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { AgentToolsContext } from "../../workbench/AgentToolsProvider";
import { ComposerAddActions } from "./ComposerAddActions";

afterEach(cleanup);

function renderActions(chatAvailable: boolean) {
  const launch = vi.fn();
  const onClose = vi.fn();
  render(
    <AgentToolsContext
      value={{ launch, chatAvailable, terminalAvailable: false }}
    >
      <ComposerAddActions
        sessionConfig={null}
        pending={false}
        disabled={false}
        onClose={onClose}
        onSetSessionMode={vi.fn()}
        onSetSessionConfigOption={vi.fn()}
      />
    </AgentToolsContext>,
  );
  return { launch, onClose };
}

it("opens the Codex CLI session picker from the Add menu of an Agent chat", () => {
  const { launch, onClose } = renderActions(true);
  fireEvent.click(
    screen.getByRole("menuitem", { name: "Open a Codex CLI session" }),
  );
  expect(onClose).toHaveBeenCalled();
  expect(launch).toHaveBeenCalledWith("sessionImport");
});

it("offers no picker where no Chat owns the launch", () => {
  renderActions(false);
  expect(
    screen.queryByRole("menuitem", { name: "Open a Codex CLI session" }),
  ).toBeNull();
});
