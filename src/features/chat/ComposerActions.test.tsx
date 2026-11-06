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

async function renderActions(
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
  const trigger = screen.queryByRole("button", { name: "Composer options" })!;
  if (trigger && !trigger.hasAttribute("disabled")) {
    fireEvent.click(trigger);
    await screen.findByRole("menu", { name: "Composer options menu" });
  }
  return {
    ...view,
    trigger,
    setMode,
    rerenderActions: (
      next: Partial<ComponentProps<typeof ChatHarnessControls>>,
    ) => view.rerender(<ChatHarnessControls {...controls.props} {...next} />),
  };
}

it("lists every advertised mode, including one whose id is default", async () => {
  const setMode = vi.fn().mockResolvedValue(undefined);
  await renderActions(true, {
    sessionConfig: decodeSessionConfig({
      sageAgentSessionId: "session-a",
      runtimeEpoch: "1",
      configRevision: "1",
      modes: {
        currentModeId: "default",
        availableModes: [
          { id: "default", name: "Default" },
          { id: "plan", name: "Plan" },
        ],
      },
      configOptions: [],
    }),
    onSetSessionMode: setMode,
  });
  const menu = screen.getByRole("menu", { name: "Composer options menu" });
  expect(within(menu).getByText("Add")).toBeInTheDocument();
  expect(
    within(menu)
      .getAllByRole("menuitemradio")
      .map((item) => item.getAttribute("aria-label")),
  ).toEqual(["Default", "Plan mode"]);
  expect(
    within(menu).getByRole("menuitemradio", { name: "Default" }),
  ).toHaveAttribute("aria-checked", "true");
  const plan = within(menu).getByRole("menuitemradio", { name: "Plan mode" });
  expect(plan).toHaveAttribute("aria-checked", "false");
  expect(plan).toHaveAccessibleDescription("Plan mode is off");
  fireEvent.click(plan);
  expect(setMode).toHaveBeenCalledExactlyOnceWith("plan");
});

it("selects an advertised session mode directly in the Add menu", async () => {
  const { trigger, setMode } = await renderActions();
  const menu = screen.getByRole("menu", { name: "Composer options menu" });
  expect(within(menu).queryByRole("combobox")).toBeNull();
  expect(within(menu).queryByRole("menuitemcheckbox")).toBeNull();
  const build = within(menu).getByRole("menuitemradio", { name: "Build" });
  const plan = within(menu).getByRole("menuitemradio", { name: "Plan mode" });
  expect(build).toHaveAttribute("aria-checked", "true");
  expect(build).not.toHaveAttribute("aria-description");
  expect(plan).toHaveAccessibleDescription("Plan mode is off");
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

it("describes Plan mode as on when selected", async () => {
  await renderActions(true, {
    sessionConfig: decodeSessionConfig({
      sageAgentSessionId: "session-a",
      runtimeEpoch: "1",
      configRevision: "1",
      modes: {
        currentModeId: "plan",
        availableModes: [{ id: "plan", name: "Plan" }],
      },
      configOptions: [],
    }),
  });
  expect(
    screen.getByRole("menuitemradio", { name: "Plan mode" }),
  ).toHaveAccessibleDescription("Plan mode is on");
});

it("sends nothing when the current mode is chosen again", async () => {
  const setMode = vi.fn().mockResolvedValue(undefined);
  const { trigger } = await renderActions(true, {
    sessionConfig: decodeSessionConfig({
      sageAgentSessionId: "session-a",
      runtimeEpoch: "1",
      configRevision: "1",
      modes: {
        currentModeId: "plan",
        availableModes: [
          { id: "default", name: "Default" },
          { id: "plan", name: "Plan" },
        ],
      },
      configOptions: [],
    }),
    onSetSessionMode: setMode,
  });
  const plan = screen.getByRole("menuitemradio", { name: "Plan mode" });
  expect(plan).toHaveAttribute("aria-checked", "true");
  fireEvent.click(plan);
  expect(setMode).not.toHaveBeenCalled();
  expect(screen.queryByRole("menu")).toBeNull();
  fireEvent.click(trigger);
  fireEvent.click(screen.getByRole("menuitemradio", { name: "Default" }));
  expect(setMode).toHaveBeenCalledExactlyOnceWith("default");
});

it("leaves a non-Plan mode only by choosing another advertised mode", async () => {
  const setMode = vi.fn().mockResolvedValue(undefined);
  const { trigger } = await renderActions(true, {
    sessionConfig: decodeSessionConfig({
      sageAgentSessionId: "session-a",
      runtimeEpoch: "1",
      configRevision: "1",
      modes: {
        currentModeId: "ask",
        availableModes: [
          { id: "manual", name: "Manual" },
          { id: "ask", name: "Ask" },
        ],
      },
      configOptions: [],
    }),
    onSetSessionMode: setMode,
  });
  fireEvent.click(screen.getByRole("menuitemradio", { name: "Ask" }));
  expect(setMode).not.toHaveBeenCalled();
  fireEvent.click(trigger);
  fireEvent.click(screen.getByRole("menuitemradio", { name: "Manual" }));
  expect(setMode).toHaveBeenCalledExactlyOnceWith("manual");
});

it("does not enable Add for effort alone while its trigger is visible", async () => {
  await renderActions(false, {
    sessionConfig: decodeSessionConfig({
      sageAgentSessionId: "session-a",
      runtimeEpoch: "1",
      configRevision: "1",
      modes: null,
      configOptions: [
        {
          id: "effort",
          name: "Effort",
          category: "effort",
          type: "select",
          currentValue: "high",
          options: [{ value: "high", name: "High" }],
        },
      ],
    }),
  });
  expect(
    screen.queryByRole("button", { name: "Composer options" }),
  ).toBeDisabled();
  expect(screen.queryByRole("menuitem", { name: "Thinking level" })).toBeNull();
});

it("keeps configuration choices on the same surface and project selection outside Add", async () => {
  const setConfig = vi.fn().mockResolvedValue(undefined);
  useWorkbenchStore.setState({
    project: { id: "a", name: "A", roots: [] },
    projects: [{ id: "a", name: "A", rootCount: 0, lastOpenedAt: 1 }],
  });
  const { trigger } = await renderActions(false, {
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
  async (currentValue) => {
    const setConfig = vi.fn().mockResolvedValue(undefined);
    const { trigger } = await renderActions(false, {
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

it("gives Add a header and icon rows with truthful one-line Plan and Goal descriptions", async () => {
  const onOpenGoal = vi.fn();
  await renderActions(true, { onOpenGoal });
  const menu = screen.getByRole("menu", { name: "Composer options menu" });
  expect(within(menu).getByText("Add", { exact: true })).toBeVisible();
  expect(within(menu).queryByText("Session mode")).toBeNull();
  const plan = within(menu).getByRole("menuitemradio", { name: "Plan mode" });
  expect(plan).toHaveAccessibleDescription("Plan mode is off");
  expect(plan.querySelector('svg[aria-hidden="true"]')).not.toBeNull();
  const goal = within(menu).getByRole("menuitem", { name: "Goal" });
  expect(goal).toHaveAccessibleDescription("View the current goal");
  expect(goal.querySelector('svg[aria-hidden="true"]')).not.toBeNull();
  fireEvent.click(goal);
  expect(onOpenGoal).toHaveBeenCalledOnce();
  expect(screen.queryByRole("menu")).toBeNull();
});

it.each(["mode", "config"])(
  "announces pending and failed %s changes outside Add, even after dismissal",
  async (kind) => {
    const { trigger } = await renderActions(kind === "mode", {
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

it("keeps Add and Harness mutually exclusive and restores plus focus on Escape", async () => {
  const { trigger } = await renderActions();
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

it("disables Add instead of opening an empty capability menu", async () => {
  const { trigger } = await renderActions(false);
  expect(trigger).toBeDisabled();
  expect(screen.queryByRole("menu")).toBeNull();
});

it("keeps Add open when a sibling Chat disables only project switching", async () => {
  useWorkbenchStore.setState({
    projects: [{ id: "a", name: "A", rootCount: 0, lastOpenedAt: 1 }],
    chatUiById: { peer: { sending: true } } as never,
  });
  await renderActions(true, { projectName: "A" });
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
  async (kind) => {
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
    const { rerenderActions } = await renderActions(false, {
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
  async (name) => {
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
    const { trigger } = await renderActions(false);
    expect(trigger).toBeDisabled();
    expect(screen.queryByRole("menu")).toBeNull();
    expect(screen.queryByRole("menuitemradio")).toBeNull();
    for (const action of ["Files", "Goal", "Plan", "Add images", "Plugins"]) {
      expect(screen.queryByText(action, { exact: true })).toBeNull();
    }
  },
);
