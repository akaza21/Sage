import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useChatAccessDrafts } from "../../app/chatAccessDrafts";
import { useChatAccessStore } from "../../app/sessionAccessActions";
import { useSessionAccessModeStore } from "../../app/sessionAccessModeStore";
import { useWorkbenchStore } from "../../app/store";
import { decodeSessionConfig } from "../../domain/acpSessionConfig";
import type { SessionAccessModeCapability } from "../../domain/sessionAccessMode";
import { resetWorkbenchPortsForTests } from "../../ports/workbenchRuntime";
import { ChatHarnessControls } from "./ChatHarnessControls";

const port = vi.hoisted(() => ({
  get: vi.fn(),
  set: vi.fn(),
  prepare: vi.fn(),
  grant: vi.fn(),
}));
vi.mock("../../ports/chatAccessChanges", () => ({
  chatAccessChanges: () => port,
}));

// Access modes that carry the harness's session modes, as a binding reports
// them; no harness name is involved.
const governed: SessionAccessModeCapability[] = [
  {
    id: "ask",
    label: "Ask each time",
    description: "Review each permission request.",
    fallback: true,
    sessionModeId: "default",
  },
  {
    id: "plan",
    label: "Plan",
    description: "Plans without changing files.",
    sessionModeId: "plan",
  },
];
const live = decodeSessionConfig({
  sageAgentSessionId: "session-1",
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
});
const initial = useWorkbenchStore.getState();

function renderControls(
  props: Partial<Parameters<typeof ChatHarnessControls>[0]>,
) {
  return render(
    <ChatHarnessControls
      availableBinding
      canPrompt
      discoveryPhase="ready"
      modelLabelScope="chat-a"
      sessionHarnessId="governed"
      sessionModelId={null}
      selectedHarnessId="governed"
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
      onSelectModel={vi.fn()}
      onModeRemoved={vi.fn()}
      {...props}
    />,
  );
}

async function planRow() {
  fireEvent.click(screen.getByRole("button", { name: "Composer options" }));
  return screen.findByRole("menuitemcheckbox", { name: "Plan mode" });
}

beforeEach(() => {
  resetWorkbenchPortsForTests();
  for (const fn of Object.values(port)) fn.mockReset();
  port.get.mockResolvedValue(null);
  port.set.mockResolvedValue(undefined);
  useWorkbenchStore.setState({
    harnesses: [
      {
        id: "governed",
        name: "Governed fixture",
        description: "",
        installState: "available",
        source: "builtIn",
        transport: "acp",
        bindingId: "governed-binding",
        accountModelDiscovery: false,
        accessModes: governed,
      },
    ],
  });
  useSessionAccessModeStore.setState({ modes: {}, ready: true, error: null });
  useChatAccessStore.setState({ modes: {}, error: null });
  useChatAccessDrafts.setState({ drafts: {} });
});
afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initial, true);
});

it("drafts the Plan access mode from Add before a new chat's session exists", async () => {
  renderControls({
    accessTarget: { bindingId: "governed-binding", draftChatId: "chat-a" },
  });
  const plan = await planRow();
  expect(plan).toHaveAttribute("aria-checked", "false");
  expect(plan).toHaveAccessibleDescription("Turn plan mode on");
  fireEvent.click(plan);
  expect(screen.queryByRole("menu")).toBeNull();
  expect(useChatAccessDrafts.getState().drafts["chat-a"]).toEqual({
    bindingId: "governed-binding",
    mode: "plan",
  });
  await waitFor(() =>
    expect(
      screen.getByRole("button", { name: "Access options" }),
    ).toHaveTextContent("Plan"),
  );

  const on = await planRow();
  expect(on).toHaveAttribute("aria-checked", "true");
  expect(on).toHaveAccessibleDescription("Turn plan mode off");
  fireEvent.click(on);
  // Off is the harness default, which needs no draft.
  expect(useChatAccessDrafts.getState().drafts["chat-a"]).toBeUndefined();
});

it("switches a live chat's access to Plan and back to its default", async () => {
  renderControls({
    sessionConfig: live,
    accessTarget: { bindingId: "governed-binding" },
  });
  const plan = await planRow();
  // The governed session modes step aside: Plan is offered once.
  expect(screen.queryByRole("menuitemradio", { name: "Plan mode" })).toBeNull();
  fireEvent.click(plan);
  await waitFor(() =>
    expect(port.set).toHaveBeenCalledExactlyOnceWith("session-1", "plan"),
  );
  await waitFor(() =>
    expect(
      screen.getByRole("button", { name: "Access options" }),
    ).toHaveTextContent("Plan"),
  );

  const on = await planRow();
  expect(on).toHaveAttribute("aria-checked", "true");
  fireEvent.click(on);
  await waitFor(() =>
    expect(port.set).toHaveBeenLastCalledWith("session-1", "ask"),
  );
});

it("confirms a default that needs confirmation before Plan turns off", async () => {
  useWorkbenchStore.setState({
    harnesses: [
      {
        ...useWorkbenchStore.getState().harnesses[0]!,
        accessModes: [
          governed[1]!,
          {
            id: "workspaceWrite",
            label: "Workspace edits",
            description: "Edits files in the workspace without asking.",
            fallback: true,
            sessionModeId: "acceptEdits",
          },
        ],
      },
    ],
  });
  port.get.mockResolvedValue("plan");
  renderControls({
    sessionConfig: decodeSessionConfig({
      sageAgentSessionId: "session-1",
      runtimeEpoch: "1",
      configRevision: "1",
      modes: {
        currentModeId: "plan",
        availableModes: [
          { id: "plan", name: "Plan" },
          { id: "acceptEdits", name: "Accept edits" },
        ],
      },
      configOptions: [],
    }),
    accessTarget: { bindingId: "governed-binding" },
  });
  const on = await planRow();
  await waitFor(() => expect(on).toHaveAttribute("aria-checked", "true"));
  fireEvent.click(on);
  // Plan off takes the access menu's own confirmation, never a direct switch.
  const dialog = await screen.findByRole("dialog", {
    name: "Switch to Workspace edits?",
  });
  expect(port.set).not.toHaveBeenCalled();
  fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(port.set).not.toHaveBeenCalled();
  expect(
    screen.getByRole("button", { name: "Access options" }),
  ).toHaveTextContent("Plan");

  fireEvent.click(await planRow());
  fireEvent.click(
    within(
      await screen.findByRole("dialog", { name: "Switch to Workspace edits?" }),
    ).getByRole("button", { name: "Switch access" }),
  );
  await waitFor(() =>
    expect(port.set).toHaveBeenCalledExactlyOnceWith(
      "session-1",
      "workspaceWrite",
    ),
  );
  // Opening the access menu afterwards shows the menu, not a confirmation.
  fireEvent.click(screen.getByRole("button", { name: "Access options" }));
  expect(
    await screen.findByRole("menu", { name: "Access mode" }),
  ).toBeInTheDocument();
  expect(screen.queryByRole("dialog")).toBeNull();
});

it("offers no access Plan row where access modes do not carry session modes", async () => {
  useWorkbenchStore.setState({
    harnesses: [
      {
        ...useWorkbenchStore.getState().harnesses[0]!,
        accessModes: governed.map(({ sessionModeId: _, ...mode }) => mode),
      },
    ],
  });
  renderControls({
    accessTarget: { bindingId: "governed-binding", draftChatId: "chat-a" },
  });
  expect(
    screen.getByRole("button", { name: "Composer options" }),
  ).toBeDisabled();
});
