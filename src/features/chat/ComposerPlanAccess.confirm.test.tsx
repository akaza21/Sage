import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
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
import { settleLazyModules } from "../../test/settleLazyModules";
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
// The access menu's body loads only once the test lets it.
const menuLoad = vi.hoisted(() => {
  let release = () => {};
  const loaded = new Promise<void>((resolve) => {
    release = resolve;
  });
  return { loaded, release: () => release() };
});
vi.mock("./ComposerAccessMenu", async (importOriginal) => {
  await menuLoad.loaded;
  return importOriginal();
});

const modes: SessionAccessModeCapability[] = [
  {
    id: "plan",
    label: "Plan",
    description: "Plans without changing files.",
    sessionModeId: "plan",
  },
  {
    id: "workspaceWrite",
    label: "Workspace edits",
    description: "Edits files in the workspace without asking.",
    fallback: true,
    sessionModeId: "acceptEdits",
  },
];
const session = (id: string) =>
  decodeSessionConfig({
    sageAgentSessionId: id,
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
  });
const initial = useWorkbenchStore.getState();

function renderChat(sessionId: string) {
  return render(
    <ChatHarnessControls
      availableBinding
      canPrompt
      discoveryPhase="ready"
      modelLabelScope={sessionId}
      sessionHarnessId="governed"
      sessionModelId={null}
      selectedHarnessId="governed"
      selectedModelId={null}
      projectName={null}
      turnBusy={false}
      sessionConfig={session(sessionId)}
      modePending={false}
      modeError={null}
      focused
      accessTarget={{ bindingId: "governed-binding" }}
      onSetSessionMode={vi.fn()}
      onSetSessionConfigOption={vi.fn()}
      onSelectHarness={vi.fn()}
      onSelectModel={vi.fn()}
      onModeRemoved={vi.fn()}
    />,
  ).container;
}

beforeEach(() => {
  resetWorkbenchPortsForTests();
  for (const fn of Object.values(port)) fn.mockReset();
  port.get.mockResolvedValue("plan");
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
        accessModes: modes,
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

it("drops a Plan-off confirmation cancelled while the access menu loads", async () => {
  const first = renderChat("session-1");
  const second = renderChat("session-2");
  fireEvent.click(
    within(first).getByRole("button", { name: "Composer options" }),
  );
  const plan = await screen.findByRole("menuitemcheckbox", {
    name: "Plan mode",
  });
  await act(async () => {
    fireEvent.click(plan);
  });
  expect(await screen.findByText("Loading options…")).toBeInTheDocument();
  fireEvent.keyDown(document.activeElement ?? document.body, {
    key: "Escape",
  });
  expect(screen.queryByText("Loading options…")).toBeNull();

  menuLoad.release();
  await settleLazyModules();
  fireEvent.click(
    within(second).getByRole("button", { name: "Access options" }),
  );
  expect(
    await screen.findByRole("menu", { name: "Access mode" }),
  ).toBeInTheDocument();
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(port.set).not.toHaveBeenCalled();
});
