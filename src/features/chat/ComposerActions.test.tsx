import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type { ComponentProps } from "react";
import { useWorkbenchStore } from "../../app/store";
import { decodeSessionConfig } from "../../domain/acpSessionConfig";
import { ChatHarnessControls } from "./ChatHarnessControls";

const initial = useWorkbenchStore.getState();
afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initial, true);
});

function renderActions(
  modes = true,
  overrides: Partial<ComponentProps<typeof ChatHarnessControls>> = {},
) {
  const setMode = vi.fn().mockResolvedValue(undefined);
  const controls = (
    <ChatHarnessControls
      availableBinding
      canPrompt
      discoveryPhase="ready"
      modelLabelScope="chat-a"
      sessionHarnessId="fixture"
      sessionModelId={null}
      selectedHarnessId="fixture"
      selectedModelId={null}
      projectName={null}
      turnBusy={false}
      modePending={false}
      modeError={null}
      focused
      sessionConfig={decodeSessionConfig({
        sageAgentSessionId: "session-a",
        runtimeEpoch: "1",
        configRevision: "1",
        modes: modes
          ? {
              currentModeId: "build",
              availableModes: [
                { id: "build", name: "Build" },
                { id: "plan-wire", name: "Plan" },
              ],
            }
          : null,
        configOptions: [],
      })}
      onSetSessionMode={setMode}
      onSetSessionConfigOption={vi.fn()}
      onSelectHarness={vi.fn()}
      onSelectModel={vi.fn()}
      onModeRemoved={vi.fn()}
      {...overrides}
    />
  );
  const view = render(controls);
  const trigger = screen.getByRole("button", { name: "Composer options" });
  fireEvent.click(trigger);
  return {
    ...view,
    trigger,
    setMode,
    rerenderActions: (
      next: Partial<ComponentProps<typeof ChatHarnessControls>>,
    ) => view.rerender(<ChatHarnessControls {...controls.props} {...next} />),
  };
}

it("selects an advertised session mode directly in the Add menu", () => {
  const { trigger, setMode } = renderActions();
  const menu = screen.getByRole("menu", { name: "Composer options menu" });
  expect(within(menu).queryByRole("combobox")).toBeNull();
  const build = within(menu).getByRole("menuitemradio", { name: "Build" });
  const plan = within(menu).getByRole("menuitemradio", { name: "Plan" });
  expect(build).toHaveAttribute("aria-checked", "true");
  expect(build).toHaveFocus();
  fireEvent.keyDown(menu, { key: "End" });
  expect(plan).toHaveFocus();
  fireEvent.keyDown(menu, { key: "Home" });
  expect(build).toHaveFocus();
  fireEvent.keyDown(menu, { key: "ArrowDown" });
  expect(plan).toHaveFocus();
  fireEvent.click(plan);
  expect(setMode).toHaveBeenCalledExactlyOnceWith("plan-wire");
  expect(screen.queryByRole("menu")).toBeNull();
  expect(trigger).toHaveFocus();
});

it("keeps configuration choices on the same surface and project selection outside Add", () => {
  const setConfig = vi.fn().mockResolvedValue(undefined);
  useWorkbenchStore.setState({
    project: { id: "a", name: "A", roots: [] },
    projects: [{ id: "a", name: "A", rootCount: 0, lastOpenedAt: 1 }],
  });
  const { trigger } = renderActions(false, {
    projectName: "A",
    onSetSessionConfigOption: setConfig,
    sessionConfig: decodeSessionConfig({
      sageAgentSessionId: "session-a",
      runtimeEpoch: "1",
      configRevision: "1",
      modes: null,
      configOptions: [
        {
          id: "speed",
          name: "Speed",
          type: "select",
          currentValue: "fast",
          options: [
            { value: "fast", name: "Fast" },
            { value: "slow", name: "Careful" },
          ],
        },
      ],
    }),
  });
  const menu = screen.getByRole("menu", { name: "Composer options menu" });
  expect(within(menu).queryByRole("button", { name: "Project: A" })).toBeNull();
  fireEvent.click(within(menu).getByRole("menuitemradio", { name: "Careful" }));
  expect(setConfig).toHaveBeenCalledExactlyOnceWith("speed", { value: "slow" });
  expect(screen.queryByRole("menu")).toBeNull();
  expect(trigger).toHaveFocus();
  fireEvent.click(screen.getByRole("button", { name: "Project: A" }));
  expect(
    screen.queryByRole("menu", { name: "Composer options menu" }),
  ).toBeNull();
  expect(screen.getAllByRole("listbox")).toHaveLength(1);
  fireEvent.click(trigger);
  expect(screen.queryByRole("listbox")).toBeNull();
});

it.each([false, true])(
  "closes Add and restores focus after changing a boolean from %s",
  (currentValue) => {
    const setConfig = vi.fn().mockResolvedValue(undefined);
    const { trigger } = renderActions(false, {
      onSetSessionConfigOption: setConfig,
      sessionConfig: decodeSessionConfig({
        sageAgentSessionId: "session-a",
        runtimeEpoch: "1",
        configRevision: "1",
        modes: null,
        configOptions: [
          { id: "flag", name: "Flag", type: "boolean", currentValue },
        ],
      }),
    });
    fireEvent.click(
      screen.getByRole("menuitemcheckbox", { name: "Session flag" }),
    );
    expect(setConfig).toHaveBeenCalledExactlyOnceWith("flag", {
      type: "boolean",
      value: !currentValue,
    });
    expect(screen.queryByRole("menu")).toBeNull();
    expect(trigger).toHaveFocus();
  },
);

it("keeps the mode group named while hiding its visual heading from menu navigation", () => {
  renderActions();
  const group = screen.getByRole("group", { name: "Session mode" });
  expect(within(group).getByText("Session mode")).toHaveAttribute(
    "aria-hidden",
    "true",
  );
});

it.each(["mode", "config"])(
  "announces pending and failed %s changes outside Add, even after dismissal",
  (kind) => {
    const { trigger } = renderActions(kind === "mode", {
      modePending: true,
      modeError: "Session settings refused",
      ...(kind === "config"
        ? {
            sessionConfig: decodeSessionConfig({
              sageAgentSessionId: "session-a",
              runtimeEpoch: "1",
              configRevision: "1",
              modes: null,
              configOptions: [
                {
                  id: "flag",
                  name: "Flag",
                  type: "boolean",
                  currentValue: false,
                },
              ],
            }),
          }
        : {}),
    });
    const menu = screen.getByRole("menu", { name: "Composer options menu" });
    const status = screen.getByRole("status");
    const error = screen.getByRole("alert");
    expect(status).toHaveTextContent("Changing session settings");
    expect(error).toHaveTextContent("Session settings refused");
    expect(menu).not.toContainElement(status);
    expect(menu).not.toContainElement(error);
    fireEvent.keyDown(menu, { key: "Escape" });
    expect(screen.queryByRole("menu")).toBeNull();
    expect(status).toBeVisible();
    expect(error).toBeVisible();
    expect(trigger).toHaveFocus();
  },
);

it("keeps Add and Harness mutually exclusive and restores plus focus on Escape", () => {
  const { trigger } = renderActions();
  const harness = screen.getByRole("button", { name: "Harness" });
  fireEvent.click(harness);
  expect(
    screen.queryByRole("menu", { name: "Composer options menu" }),
  ).toBeNull();
  expect(screen.getAllByRole("listbox")).toHaveLength(1);
  fireEvent.click(trigger);
  expect(screen.queryByRole("listbox")).toBeNull();
  const menu = screen.getByRole("menu", { name: "Composer options menu" });
  fireEvent.keyDown(menu, { key: "Escape" });
  expect(screen.queryByRole("menu")).toBeNull();
  expect(trigger).toHaveFocus();
});

it("disables Add instead of opening an empty capability menu", () => {
  const { trigger } = renderActions(false);
  expect(trigger).toBeDisabled();
  expect(screen.queryByRole("menu")).toBeNull();
});

it("keeps Add open when a sibling Chat disables only project switching", async () => {
  useWorkbenchStore.setState({
    projects: [{ id: "a", name: "A", rootCount: 0, lastOpenedAt: 1 }],
    chatUiById: { peer: { sending: true } } as never,
  });
  renderActions(true, { projectName: "A" });
  await waitFor(() =>
    expect(
      screen.getByRole("menu", { name: "Composer options menu" }),
    ).toBeVisible(),
  );
  expect(screen.getByRole("button", { name: "Project: A" })).toBeDisabled();
  expect(screen.getByRole("menuitemradio", { name: "Build" })).toHaveFocus();
});

it.each(["image", "goal", "config"])(
  "dismisses Add when its focused %s action is withdrawn",
  (kind) => {
    const onModeRemoved = vi.fn();
    const config = decodeSessionConfig({
      sageAgentSessionId: "session-a",
      runtimeEpoch: "1",
      configRevision: "1",
      modes: null,
      configOptions: [
        { id: "flag", name: "Flag", type: "boolean", currentValue: false },
      ],
    });
    const { rerenderActions } = renderActions(false, {
      onModeRemoved,
      ...(kind === "image"
        ? { imageAction: { disabled: false, onChoose: vi.fn() } }
        : kind === "goal"
          ? { onOpenGoal: vi.fn() }
          : { sessionConfig: config }),
    });
    expect(document.activeElement).not.toBe(document.body);
    rerenderActions({
      imageAction: undefined,
      onOpenGoal: undefined,
      sessionConfig: null,
    });
    expect(screen.queryByRole("menu")).toBeNull();
    expect(onModeRemoved).toHaveBeenCalledOnce();
  },
);

it.each(["Claude", "Codex", "Pi", "OpenCode", "OMP"])(
  "omits unsupported Add actions for %s rather than inferring from its name",
  (name) => {
    useWorkbenchStore.setState({
      harnesses: [
        {
          id: "fixture",
          name,
          description: "",
          installState: "available",
          source: "builtIn",
          transport: "acp",
          bindingId: "fixture",
          accountModelDiscovery: false,
        },
      ],
    });
    const { trigger } = renderActions(false);
    expect(trigger).toBeDisabled();
    expect(screen.queryByRole("menu")).toBeNull();
    expect(screen.queryByRole("menuitemradio")).toBeNull();
    for (const action of ["Files", "Goal", "Plan", "Add images", "Plugins"]) {
      expect(screen.queryByText(action, { exact: true })).toBeNull();
    }
  },
);
