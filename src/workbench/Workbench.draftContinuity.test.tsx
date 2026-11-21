import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { clearAcpProjectionRegistry } from "../app/acpProjectionRegistry";
import { useWorkbenchStore } from "../app/store";
import { useAgentSettingsState } from "../features/settings/agentSettingsState";
import { resetBrowserDesktopForTests } from "../services/browserDesktop";
import { settleLazyModules } from "../test/settleLazyModules";
import { Workbench } from "./Workbench";
import { useSideChatState } from "./sideChatState";

// Isolate this mounted race so the draft actions chunk is not already cached.
const actionsLoad = vi.hoisted(() => {
  let release!: () => void;
  const ready = new Promise<void>((resolve) => {
    release = resolve;
  });
  return { ready, release };
});
vi.mock("../app/newChatDraftActions", async (importOriginal) => {
  await actionsLoad.ready;
  return importOriginal();
});

const initial = useWorkbenchStore.getState();

beforeEach(async () => {
  localStorage.clear();
  resetBrowserDesktopForTests();
  useWorkbenchStore.setState(initial, true);
  vi.stubGlobal("innerWidth", 1800);
  vi.stubGlobal(
    "matchMedia",
    vi.fn(() => ({
      matches: false,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    })),
  );
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
  await useWorkbenchStore.getState().initialize();
});

afterEach(() => {
  cleanup();
  clearAcpProjectionRegistry();
  useSideChatState.setState({ sides: [], pairedId: null });
  useAgentSettingsState.getState().close();
  vi.unstubAllGlobals();
  useWorkbenchStore.setState(initial, true);
  resetBrowserDesktopForTests();
  localStorage.clear();
});

it("opens an editable draft after switching to an empty project while New chat actions load", async () => {
  const state = useWorkbenchStore.getState();
  const originalProjectId = state.project!.id;
  expect(
    await state.createProject({ name: "Empty project", sourceTokens: [] }),
  ).toBe(true);
  const emptyProjectId = useWorkbenchStore.getState().project!.id;
  await state.switchProject(originalProjectId);
  render(<Workbench />);
  await settleLazyModules();

  try {
    fireEvent.click(screen.getByRole("button", { name: "New chat" }));
    expect(useWorkbenchStore.getState().newChatDraftOpening).toBe(true);
    await act(async () => state.switchProject(emptyProjectId));
    expect(useWorkbenchStore.getState().project?.id).toBe(emptyProjectId);
    expect(useWorkbenchStore.getState().tabs).toEqual([]);
    expect(useWorkbenchStore.getState().newChatDraft).toBeNull();
    expect(useWorkbenchStore.getState().newChatDraftOpening).toBe(true);
  } finally {
    actionsLoad.release();
    await settleLazyModules();
  }

  const draft = screen.getByRole("region", { name: "New chat draft" });
  const composer = within(draft).getByRole("combobox", {
    name: "Message agent",
  });
  expect(composer).toBeEnabled();
  fireEvent.change(composer, { target: { value: "Work in this project" } });
  expect(composer).toHaveValue("Work in this project");
  expect(useWorkbenchStore.getState().newChatDraft).toMatchObject({
    projectId: emptyProjectId,
    targetProjectId: emptyProjectId,
    text: "Work in this project",
  });
});
