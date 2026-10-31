import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useSessionAccessModeStore } from "../../app/sessionAccessModeStore";
import { useWorkbenchStore } from "../../app/store";
import { useUiAppearanceStore } from "../../app/uiAppearanceStore";
import type { SessionAccessModes } from "../../domain/sessionAccessMode";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import { AgentSettingsSurface } from "./AgentSettingsSurface";
import { useAgentSettingsState } from "./agentSettingsState";

const workbench = useWorkbenchStore.getState();
const access = useSessionAccessModeStore.getState();
const appearance = useUiAppearanceStore.getState();
beforeEach(async () => {
  useAgentSettingsState.getState().open("Chats");
  useWorkbenchStore.setState({
    project: { id: "project-a", name: "A", roots: [] },
    projectTransitioning: false,
    selectedHarnessId: "adapter-a",
    harnessDiscoveryPhase: "ready",
    harnesses: [
      {
        id: "adapter-a",
        name: "Test harness",
        description: "Discovered adapter",
        source: "builtIn",
        transport: "acp",
        installState: "available",
        bindingId: "binding-a",
        accountModelDiscovery: false,
        accessModes: [
          {
            id: "readOnly",
            label: "Read only",
            description: "Read files",
            fallback: true,
          },
        ],
      },
    ],
    models: [{ id: "model-a", name: "Advertised model" }],
    selectedModelId: "model-a",
  });
  useSessionAccessModeStore.setState({ ready: true, modes: {}, error: null });
  const initialSettings = render(<AgentSettingsSurface />);
  await screen.findByRole("combobox", { name: "Current model" });
  initialSettings.unmount();
});

it("shows installation and sign-in evidence without treating an installed adapter as signed in", () => {
  render(<AgentSettingsSurface />);
  fireEvent.click(screen.getByRole("button", { name: "Harnesses" }));
  expect(screen.getByText("Test harness")).toBeVisible();
  expect(screen.getByText(/Sign-in status is not exposed/)).toBeVisible();
  expect(screen.queryByText("Signed in")).toBeNull();
});

it("restores an archived chat through the existing project-scoped action", async () => {
  const restoreChat = vi.fn(async () => true);
  useWorkbenchStore.setState({
    archivedChats: [{ id: "archived-a", title: "Archived work" }],
    restoreChat,
  });
  render(<AgentSettingsSurface />);
  fireEvent.click(screen.getByRole("button", { name: "Archived chats" }));
  fireEvent.click(
    screen.getByRole("button", { name: "Unarchive Archived work" }),
  );
  await waitFor(() =>
    expect(restoreChat).toHaveBeenCalledWith("archived-a", "project-a"),
  );
  expect(await screen.findByText("Chat unarchived.")).toBeVisible();
});

it("uses persisted appearance settings and leaves Editor appearance separate", () => {
  const setAppearance = vi.fn();
  useUiAppearanceStore.setState({
    ready: true,
    error: null,
    appearance: { fontSize: 14, density: "comfortable" },
    setAppearance,
  });
  render(<AgentSettingsSurface />);
  fireEvent.click(screen.getByRole("button", { name: "Appearance" }));
  fireEvent.change(
    screen.getByRole("combobox", { name: "Interface font size" }),
    { target: { value: "16" } },
  );
  expect(setAppearance).toHaveBeenCalledWith({
    fontSize: 16,
    density: "comfortable",
  });
  fireEvent.change(
    screen.getByRole("combobox", { name: "Interface density" }),
    { target: { value: "compact" } },
  );
  expect(setAppearance).toHaveBeenCalledWith({
    fontSize: 14,
    density: "compact",
  });
  expect(screen.queryByRole("checkbox", { name: "Code folding" })).toBeNull();
});

it("lists app-wide and Agent shortcuts and leaves Editor shortcuts to Editor settings", async () => {
  render(<AgentSettingsSurface />);
  fireEvent.click(screen.getByRole("button", { name: "Keybindings" }));
  expect(
    (await screen.findByText("Open settings")).closest("tr"),
  ).toHaveTextContent("Ctrl+,");
  expect(screen.getByText("Find in chat")).toBeVisible();
  expect(screen.queryByText("Save file")).toBeNull();
  expect(screen.queryByRole("group", { name: "Editor" })).toBeNull();
});

it("lists every implemented browser shortcut with its browser-only scope", async () => {
  render(<AgentSettingsSurface />);
  fireEvent.click(screen.getByRole("button", { name: "Keybindings" }));
  await screen.findByText("New browser tab");
  for (const [label, chord] of [
    ["Focus browser address", "Ctrl+L"],
    ["New browser tab", "Ctrl+T"],
    ["Close browser tab", "Ctrl+W"],
    ["Reload browser page", "Ctrl+R"],
    ["Reload browser page (function key)", "F5"],
    ["Find in browser page", "Ctrl+F"],
    ["Browser back", "Alt+Left"],
    ["Browser forward", "Alt+Right"],
    ["Browser zoom in", "Ctrl++"],
    ["Browser zoom out", "Ctrl+-"],
    ["Reset browser zoom", "Ctrl+0"],
    ["Stop loading browser page", "Esc"],
    ["Native page zoom in", "Ctrl+="],
    ["Native page zoom out", "Ctrl+Subtract"],
    ["Leave browser annotation", "Esc"],
  ])
    expect(screen.getByText(label).closest("tr")).toHaveTextContent(chord);
  expect(
    screen.getByText(/Only while focus is inside the browser pane/),
  ).toBeVisible();
  expect(
    screen.getByText(/Esc leaves annotation mode first, even while idle/),
  ).toBeVisible();
});
afterEach(() => {
  cleanup();
  useAgentSettingsState.getState().close();
  useWorkbenchStore.setState(workbench, true);
  useSessionAccessModeStore.setState(access, true);
  useUiAppearanceStore.setState(appearance, true);
  vi.restoreAllMocks();
});

it("labels transient harness/model selection honestly and exposes only real access defaults", () => {
  const selectHarness = vi.fn(async () => undefined);
  const selectModel = vi.fn();
  const setMode = vi.fn();
  useWorkbenchStore.setState({ selectHarness, selectModel });
  useSessionAccessModeStore.setState({ setMode });
  render(<AgentSettingsSurface />);
  fireEvent.change(screen.getByRole("combobox", { name: "Current harness" }), {
    target: { value: "adapter-a" },
  });
  expect(selectHarness).toHaveBeenCalledWith("adapter-a");
  fireEvent.change(screen.getByRole("combobox", { name: "Current model" }), {
    target: { value: "model-a" },
  });
  expect(selectModel).toHaveBeenCalledWith("model-a");
  fireEvent.change(screen.getByRole("combobox", { name: "Default access" }), {
    target: { value: "readOnly" },
  });
  expect(setMode).toHaveBeenCalledWith("binding-a", "readOnly");
  expect(
    screen.queryByRole("combobox", { name: /Default (harness|model|effort)/ }),
  ).toBeNull();
  expect(screen.getByText(/not a saved app-wide default/)).toBeVisible();
  expect(screen.queryByRole("option", { name: "High" })).toBeNull();
});

it("keeps duplicate readable models distinct and finds models beyond the first page", () => {
  useWorkbenchStore.setState({
    models: Array.from({ length: 205 }, (_, index) => ({
      id: `model-${index}`,
      name: index < 2 ? "gpt-5" : `Model ${index}`,
    })),
    selectedModelId: "model-204",
    selectModel: (id) => useWorkbenchStore.setState({ selectedModelId: id }),
  });
  const first = render(<AgentSettingsSurface />);
  const models = screen.getByRole("combobox", { name: "Current model" });
  expect(models).toHaveValue("model-204");
  expect(screen.getByRole("option", { name: "GPT-5 (model-0)" })).toBeVisible();
  fireEvent.change(models, { target: { value: "model-1" } });
  expect(models).toHaveValue("model-1");
  fireEvent.change(screen.getByRole("searchbox", { name: "Search models" }), {
    target: { value: "Model 203" },
  });
  fireEvent.change(models, { target: { value: "model-203" } });
  expect(models).toHaveValue("model-203");
  first.unmount();
  render(<AgentSettingsSurface />);
  expect(screen.getByRole("combobox", { name: "Current model" })).toHaveValue(
    "model-203",
  );
});

it("shows only the sections T3 Code's settings also have, in its order", () => {
  render(<AgentSettingsSurface />);
  const nav = screen.getByRole("navigation", {
    name: "Agent settings sections",
  });
  expect(
    Array.from(nav.querySelectorAll("button"), (button) => button.textContent),
  ).toEqual([
    "Appearance",
    "Keybindings",
    "Harnesses",
    "Chats",
    "Browser",
    "Archived chats",
  ]);
  expect(screen.getByRole("heading", { name: "Agents" })).toBeVisible();
});

it("does not offer reserved shortcuts", async () => {
  render(<AgentSettingsSurface />);
  fireEvent.click(screen.getByRole("button", { name: "Keybindings" }));
  await screen.findByText("New browser tab");
  expect(screen.queryByText("Quick Open")).toBeNull();
});

it("closes from its close button and Escape, and Ctrl+, keeps its section", () => {
  render(<AgentSettingsSurface />);
  fireEvent.click(screen.getByRole("button", { name: "Close settings" }));
  expect(useAgentSettingsState.getState().page).toBeNull();
  useAgentSettingsState.getState().open("Chats");
  fireEvent.keyDown(screen.getByRole("button", { name: "Chats" }), {
    key: "Escape",
  });
  expect(useAgentSettingsState.getState().page).toBeNull();
  useAgentSettingsState.getState().open("Browser", "tab-a");
  useAgentSettingsState.getState().open();
  expect(useAgentSettingsState.getState()).toMatchObject({
    page: "Browser",
    browserTabId: "tab-a",
  });
});

it("restores the real per-harness access default from its persistence boundary after renderer state is discarded", async () => {
  let stored: SessionAccessModes = {};
  const settings = getWorkbenchPorts().settings;
  vi.spyOn(settings, "getSessionAccessModes").mockImplementation(
    async () => stored,
  );
  vi.spyOn(settings, "setSessionAccessModes").mockImplementation(
    async (modes) => {
      stored = modes;
      return modes;
    },
  );
  useWorkbenchStore.setState((state) => ({
    harnesses: [
      {
        ...state.harnesses[0],
        accessModes: [
          {
            id: "readOnly",
            label: "Read only",
            description: "Read",
            fallback: true,
          },
          {
            id: "workspaceWrite",
            label: "Workspace edits",
            description: "Write workspace files",
          },
        ],
      },
    ],
  }));
  await useSessionAccessModeStore.getState().restore();
  const first = render(<AgentSettingsSurface />);
  fireEvent.change(screen.getByRole("combobox", { name: "Default access" }), {
    target: { value: "workspaceWrite" },
  });
  expect(stored).toEqual({});
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(screen.getByRole("combobox", { name: "Default access" })).toHaveValue(
    "readOnly",
  );
  fireEvent.change(screen.getByRole("combobox", { name: "Default access" }), {
    target: { value: "workspaceWrite" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Switch access" }));
  await useSessionAccessModeStore.getState().settle();
  first.unmount();
  useSessionAccessModeStore.setState({ ready: false, modes: {} });
  await useSessionAccessModeStore.getState().restore();
  render(<AgentSettingsSurface />);
  expect(screen.getByRole("combobox", { name: "Default access" })).toHaveValue(
    "workspaceWrite",
  );
  expect(stored).toEqual({ "binding-a": "workspaceWrite" });
});

it("filters settings navigation without changing the current section", () => {
  render(<AgentSettingsSurface />);
  fireEvent.change(
    screen.getByRole("searchbox", { name: "Search Agent settings sections" }),
    {
      target: { value: "BROWSER" },
    },
  );
  expect(screen.getByRole("button", { name: "Browser" })).toBeVisible();
  expect(screen.queryByRole("button", { name: "Harnesses" })).toBeNull();
  expect(screen.getByRole("heading", { name: "Chats" })).toBeVisible();
  fireEvent.change(
    screen.getByRole("searchbox", { name: "Search Agent settings sections" }),
    {
      target: { value: "does not match" },
    },
  );
  expect(screen.getByText("No matching sections.")).toBeVisible();
});

it.each([
  ["font", "Interface font size"],
  ["density", "Interface density"],
])(
  "searches for %s and focuses its implemented control",
  async (query, label) => {
    useUiAppearanceStore.setState({ ready: true });
    render(<AgentSettingsSurface />);
    fireEvent.change(
      screen.getByRole("searchbox", { name: "Search Agent settings sections" }),
      {
        target: { value: query },
      },
    );
    fireEvent.click(
      screen.getByRole("button", { name: `Appearance: ${label}` }),
    );
    await waitFor(() =>
      expect(screen.getByRole("combobox", { name: label })).toHaveFocus(),
    );
  },
);

it("finds access while leaving unsupported history unavailable", async () => {
  render(<AgentSettingsSurface />);
  fireEvent.change(
    screen.getByRole("searchbox", { name: "Search Agent settings sections" }),
    {
      target: { value: "access" },
    },
  );
  fireEvent.click(
    screen.getByRole("button", { name: "Chats: Default access" }),
  );
  await waitFor(() =>
    expect(
      screen.getByRole("combobox", { name: "Default access" }),
    ).toHaveFocus(),
  );
  fireEvent.change(
    screen.getByRole("searchbox", { name: "Search Agent settings sections" }),
    {
      target: { value: "history" },
    },
  );
  expect(
    screen.getByRole("button", { name: "Browser: Browsing history" }),
  ).toBeDisabled();
  expect(screen.queryByRole("button", { name: /notification/i })).toBeNull();
});

it("discards workspace confirmation when the selected harness changes", async () => {
  const harness = useWorkbenchStore.getState().harnesses[0];
  const modes = [
    ...harness.accessModes!,
    {
      id: "workspaceWrite" as const,
      label: "Workspace edits",
      description: "Write workspace files",
    },
  ];
  useWorkbenchStore.setState({
    harnesses: [
      { ...harness, accessModes: modes },
      {
        ...harness,
        id: "adapter-b",
        bindingId: "binding-b",
        transport: "acp",
        installState: "available",
        accessModes: modes,
      },
    ],
  });
  render(<AgentSettingsSurface />);
  fireEvent.change(screen.getByRole("combobox", { name: "Default access" }), {
    target: { value: "workspaceWrite" },
  });
  expect(
    screen.getByRole("dialog", { name: "Switch to Workspace edits?" }),
  ).toBeVisible();
  const { act } = await import("@testing-library/react");
  await act(async () =>
    useWorkbenchStore.setState({ selectedHarnessId: "adapter-b" }),
  );
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(useSessionAccessModeStore.getState().modes).toEqual({});
});

it("restores the prior confirmed access after a failed save and offers a confirmed retry", async () => {
  const harness = useWorkbenchStore.getState().harnesses[0];
  useWorkbenchStore.setState({
    harnesses: [
      {
        ...harness,
        accessModes: [
          ...harness.accessModes!,
          {
            id: "workspaceWrite",
            label: "Workspace edits",
            description: "Write workspace files",
          },
        ],
      },
    ],
  });
  const settings = getWorkbenchPorts().settings;
  vi.spyOn(settings, "getSessionAccessModes").mockResolvedValue({
    "binding-a": "readOnly",
  });
  vi.spyOn(settings, "setSessionAccessModes")
    .mockRejectedValueOnce(new Error("disk unavailable"))
    .mockImplementation(async (modes) => modes);
  await useSessionAccessModeStore.getState().restore();
  render(<AgentSettingsSurface />);
  fireEvent.change(screen.getByRole("combobox", { name: "Default access" }), {
    target: { value: "workspaceWrite" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Switch access" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Permission mode could not be saved.",
  );
  expect(screen.getByRole("combobox", { name: "Default access" })).toHaveValue(
    "readOnly",
  );
  fireEvent.click(screen.getByRole("button", { name: "Retry access save" }));
  fireEvent.click(screen.getByRole("button", { name: "Switch access" }));
  await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
  expect(useSessionAccessModeStore.getState().modes).toEqual({
    "binding-a": "workspaceWrite",
  });
});

it("keeps failed restores visible and sends foreign project identity to unarchive", async () => {
  const restoreChat = vi.fn(async () => false);
  useWorkbenchStore.setState({
    restoreChat,
    error: "Native restore refused",
    projects: [
      {
        id: "foreign",
        name: "Other project",
        rootCount: 0,
        lastOpenedAt: 0,
        railArchived: [{ id: "chat-other", title: "Other archived chat" }],
      },
    ],
  });
  render(<AgentSettingsSurface />);
  fireEvent.click(screen.getByRole("button", { name: "Archived chats" }));
  fireEvent.change(
    screen.getByRole("combobox", { name: "Archived chats project" }),
    { target: { value: "foreign" } },
  );
  fireEvent.click(
    screen.getByRole("button", { name: "Unarchive Other archived chat" }),
  );
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Native restore refused",
  );
  expect(restoreChat).toHaveBeenCalledWith("chat-other", "foreign");
  expect(
    screen.getByRole("button", { name: "Unarchive Other archived chat" }),
  ).toBeEnabled();
  expect(screen.queryByText("Chat unarchived.")).toBeNull();
});

it("paginates archived chats without mounting more than fifty rows", () => {
  useWorkbenchStore.setState({
    archivedChats: Array.from({ length: 51 }, (_, index) => ({
      id: `archived-${index}`,
      title: `Archived ${index}`,
    })),
  });
  render(<AgentSettingsSurface />);
  fireEvent.click(screen.getByRole("button", { name: "Archived chats" }));
  expect(screen.getAllByRole("button", { name: /^Unarchive / })).toHaveLength(
    50,
  );
  expect(
    screen.queryByRole("button", { name: "Unarchive Archived 50" }),
  ).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Next chats" }));
  expect(screen.getAllByRole("button", { name: /^Unarchive / })).toHaveLength(
    1,
  );
  expect(
    screen.getByRole("button", { name: "Unarchive Archived 50" }),
  ).toBeVisible();
});

it("never substitutes a permissive mode when the selected harness advertises no access modes", () => {
  const harness = useWorkbenchStore.getState().harnesses[0];
  useWorkbenchStore.setState({ harnesses: [{ ...harness, accessModes: [] }] });
  render(<AgentSettingsSurface />);
  expect(
    screen.getByRole("combobox", { name: "Default access" }),
  ).toBeDisabled();
  expect(screen.queryByRole("option", { name: "Workspace edits" })).toBeNull();
});

it("does not apply an obsolete discovery result after leaving and reopening Harnesses", async () => {
  const oldInventory = useWorkbenchStore.getState().harnesses;
  const newerInventory = [{ ...oldInventory[0], name: "New discovery" }];
  let finishOld!: (value: typeof oldInventory) => void;
  vi.spyOn(getWorkbenchPorts().agent, "listHarnesses")
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finishOld = resolve;
        }),
    )
    .mockResolvedValueOnce(newerInventory);
  const apply = vi.fn(async (harnesses: typeof oldInventory) => {
    useWorkbenchStore.setState({ harnesses });
  });
  useWorkbenchStore.setState({ applyHarnessDiscovery: apply });
  render(<AgentSettingsSurface />);
  fireEvent.click(screen.getByRole("button", { name: "Harnesses" }));
  fireEvent.click(screen.getByRole("button", { name: "Refresh discovery" }));
  fireEvent.click(screen.getByRole("button", { name: "Chats" }));
  fireEvent.click(screen.getByRole("button", { name: "Harnesses" }));
  fireEvent.click(screen.getByRole("button", { name: "Refresh discovery" }));
  await waitFor(() => expect(apply).toHaveBeenCalledWith(newerInventory));
  const { act } = await import("@testing-library/react");
  await act(async () => {
    finishOld(oldInventory);
  });
  expect(apply).toHaveBeenCalledOnce();
});

it("keeps native discovery updates that supersede a pending refresh", async () => {
  const oldInventory = useWorkbenchStore.getState().harnesses;
  const nativeInventory = [{ ...oldInventory[0], name: "Native discovery" }];
  let finishRefresh!: (value: typeof oldInventory) => void;
  vi.spyOn(getWorkbenchPorts().agent, "listHarnesses").mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finishRefresh = resolve;
      }),
  );
  const apply = vi.fn(async () => undefined);
  useWorkbenchStore.setState({ applyHarnessDiscovery: apply });
  render(<AgentSettingsSurface />);
  fireEvent.click(screen.getByRole("button", { name: "Harnesses" }));
  fireEvent.click(screen.getByRole("button", { name: "Refresh discovery" }));
  const { act } = await import("@testing-library/react");
  await act(async () => {
    useWorkbenchStore.setState({ harnesses: nativeInventory });
    finishRefresh(oldInventory);
  });
  expect(apply).not.toHaveBeenCalled();
  expect(screen.getByText("Native discovery")).toBeVisible();
  expect(
    screen.getByRole("button", { name: "Refresh discovery" }),
  ).toBeEnabled();
});

it("allows Escape from a checkbox but keeps text-entry edits open", () => {
  render(<AgentSettingsSurface />);
  const dialog = screen.getByRole("region", { name: "Agent settings" });
  expect(dialog).not.toHaveAttribute("aria-modal");
  const text = document.createElement("input");
  text.type = "text";
  dialog.append(text);
  fireEvent.keyDown(text, { key: "Escape" });
  expect(useAgentSettingsState.getState().page).toBe("Chats");
  const checkbox = document.createElement("input");
  checkbox.type = "checkbox";
  dialog.append(checkbox);
  fireEvent.keyDown(checkbox, { key: "Escape" });
  expect(useAgentSettingsState.getState().page).toBeNull();
});
