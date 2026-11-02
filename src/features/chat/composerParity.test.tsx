import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { readFile } from "node:fs/promises";
import { afterEach, expect, it, vi } from "vitest";
import { useWorkbenchStore } from "../../app/store";
import { ComposerAddActions } from "./ComposerAddActions";
import { ChatHome } from "./ChatHome";
import { ChatHarnessControls } from "./ChatHarnessControls";
import { ChatMarkdown } from "./acp/ChatMarkdown";

const initial = useWorkbenchStore.getState();
const styles: HTMLStyleElement[] = [];
afterEach(() => {
  cleanup();
  styles.splice(0).forEach((style) => style.remove());
  useWorkbenchStore.setState(initial, true);
});
async function loadStyle(path: string) {
  const style = document.createElement("style");
  style.textContent = await readFile(path, "utf8");
  document.head.append(style);
  styles.push(style);
}

it("keeps Add descriptions on a secondary line without uppercasing its heading", async () => {
  await loadStyle("src/styles/harness-picker.css");
  render(
    <ComposerAddActions
      sessionConfig={{
        sageAgentSessionId: "session",
        runtimeEpoch: "1",
        configRevision: "1",
        modes: {
          currentModeId: "plan",
          availableModes: [{ id: "plan", name: "Plan" }],
        },
        configOptions: [],
      }}
      pending={false}
      disabled={false}
      onClose={() => {}}
      onSetSessionMode={vi.fn()}
      onSetSessionConfigOption={vi.fn()}
    />,
  );
  const plan = screen.getByRole("menuitemradio", { name: "Plan mode" });
  expect(getComputedStyle(plan).display).toBe("grid");
  const description = screen.getByText("Plan mode is on");
  expect(getComputedStyle(description).gridColumn).toBe("2 / -1");
  expect(getComputedStyle(description).whiteSpace).toBe("normal");
  expect(getComputedStyle(screen.getByText("Add")).textTransform).toBe("none");
});

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

it("lets the complete home project caption reflow instead of ellipsizing", async () => {
  await loadStyle("src/styles/shared-controls.css");
  await loadStyle("src/features/chat/chat-home.css");
  useWorkbenchStore.setState({
    project: { id: "project", name: "Project with a complete name", roots: [] },
    projects: [
      {
        id: "project",
        name: "Project with a complete name",
        rootCount: 0,
        lastOpenedAt: 1,
      },
    ],
  });
  render(<ChatHome projectName="Project with a complete name" />);
  const label = screen.getByText("Project with a complete name");
  expect(getComputedStyle(label).whiteSpace).toBe("normal");
  expect(getComputedStyle(label).overflow).toBe("visible");
});

it("sizes chat table columns to content before wrapping overflow", async () => {
  await loadStyle("src/features/chat/acp/acp-chat.css");
  await loadStyle("src/features/chat/acp/chat-markdown.css");
  render(
    <ChatMarkdown
      text={
        "| Status | Evidence |\n| --- | --- |\n| OK | `abcdefghijklmnopqrstuvwxyz0123456789` |\n| Long | ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 |"
      }
    />,
  );
  const table = screen.getByRole("table");
  expect(getComputedStyle(table).tableLayout).toBe("auto");
  const unbroken = screen.getByRole("cell", {
    name: "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789",
  });
  expect(getComputedStyle(unbroken).overflowWrap).toBe("anywhere");
  for (const cell of screen.getAllByRole("cell")) {
    expect(getComputedStyle(cell).minWidth).toBe("0px");
    expect(getComputedStyle(cell).whiteSpace).toBe("normal");
  }
  // Words set each column's minimum, so auto layout never splits "DuckDB".
  for (const name of ["OK", "Long"])
    expect(
      getComputedStyle(screen.getByRole("cell", { name })).overflowWrap,
    ).toBe("break-word");
  // Only an unbreakable run (plain or code) may break anywhere.
  expect(getComputedStyle(screen.getByText(/^abcdefghij/)).overflowWrap).toBe(
    "anywhere",
  );
});

it("keeps the plus target at least 24px before container overrides", async () => {
  await loadStyle("src/styles/chat-surface.css");
  render(<button className="composer__plus">Add</button>);
  expect(
    parseFloat(getComputedStyle(screen.getByRole("button")).width),
  ).toBeGreaterThanOrEqual(24);
});

it("paints a harness ring only after keyboard navigation, not initial or pointer focus", async () => {
  await loadStyle("src/styles/global.css");
  await loadStyle("src/styles/harness-picker.css");
  render(
    <ChatHarnessControls
      availableBinding
      canPrompt
      discoveryPhase="ready"
      modelLabelScope="chat"
      sessionHarnessId={null}
      sessionModelId={null}
      selectedHarnessId={null}
      selectedModelId={null}
      projectName={null}
      turnBusy={false}
      sessionConfig={null}
      modePending={false}
      modeError={null}
      focused
      onSetSessionMode={vi.fn()}
      onSetSessionConfigOption={vi.fn()}
      onSelectHarness={vi.fn()}
      onSelectModel={() => {}}
      onModeRemoved={() => {}}
    />,
  );
  const harness = screen.getByRole("button", { name: "Harness" });
  harness.focus();
  expect(getComputedStyle(harness).outline).toBe("none");
  fireEvent.keyDown(window, { key: "Tab" });
  expect(getComputedStyle(harness).outline).not.toBe("none");
  fireEvent.pointerDown(harness);
  expect(getComputedStyle(harness).outline).toBe("none");
});
