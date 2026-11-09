import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type { ComponentProps } from "react";

import { ChatComposerRegion } from "./ChatComposerRegion";
import { useWorkbenchStore } from "../../app/store";
import {
  acpProjectionStoreFor,
  clearAcpProjectionRegistry,
} from "../../app/acpProjectionRegistry";
import {
  agentSessionFixture,
  capabilitySetFixture,
} from "../../domain/acpProjection.fixture";

// Holds the Goal chunk's import until the test lets it finish.
const chunk = vi.hoisted(() => {
  let release = () => undefined as void;
  const gate = new Promise<void>((resolve) => (release = resolve));
  return { gate, release: () => release() };
});
vi.mock("./BoundChatGoalControl", async (importOriginal) => {
  await chunk.gate;
  return importOriginal();
});

const initial = useWorkbenchStore.getState();
afterEach(() => {
  cleanup();
  clearAcpProjectionRegistry();
  useWorkbenchStore.setState(initial, true);
});

const goal = {
  kind: "goal" as const,
  snapshot: {
    objective: "Ship safely",
    status: "active" as const,
    tokenBudget: null,
    tokensUsed: 0,
    timeUsedSeconds: 1,
    createdAt: null,
    updatedAt: null,
  },
};

function mutableGoalChat() {
  useWorkbenchStore.setState({
    project: { id: "project-a", name: "A", roots: [] },
    projectGeneration: 0,
    projectTransitioning: false,
    activeTabId: "chat-a",
    tabs: [
      {
        id: "chat-a",
        kind: "chat",
        title: "A",
        position: 0,
        pinned: false,
        dirty: false,
      },
    ],
  });
  acpProjectionStoreFor("chat-a").setState({
    scope: {
      projectId: "project-a",
      projectGeneration: "0",
      chatId: "chat-a",
    } as never,
    page: {
      projectId: "project-a",
      chatId: "chat-a",
      title: "A",
      projectionRevision: "1",
      nextCursor: null,
      turns: [],
      goal,
      currentSessionId: "session-a",
      sessions: [
        agentSessionFixture({
          id: "session-a" as never,
          runtimeEpoch: "1" as never,
          capabilities: capabilitySetFixture({
            protocolVersion: "codex-app-server-observed-0.167.0",
          }),
        }),
      ],
    } as never,
  });
}

const props: ComponentProps<typeof ChatComposerRegion> = {
  composerRef: { current: null },
  fieldId: "message",
  dictationScope: "chat-a",
  descriptionId: "description",
  disabledReasonId: "disabled-reason",
  text: "",
  attachedContext: [],
  enabled: true,
  actionBusy: false,
  activeTurn: false,
  projectTransitioning: false,
  workbenchError: null,
  chatError: null,
  availableBinding: true,
  canPrompt: true,
  discoveryPhase: "ready",
  sessionHarnessId: null,
  sessionModelId: null,
  selectedHarnessId: null,
  selectedModelId: null,
  sessionPromptUnavailable: false,
  sessionConfig: null,
  modePending: false,
  modeError: null,
  focused: true,
  onSetSessionMode: vi.fn(),
  onSetSessionConfigOption: vi.fn(),
  onSelectHarness: vi.fn(),
  onSelectModel: vi.fn(),
  missingBindingNotice: false,
  disabledReason: null,
  project: { name: "Project" },
  usage: null,
  goal,
  onSubmit: vi.fn(),
  onTextChange: vi.fn(),
  onRemoveContext: vi.fn(),
  onCancelTurn: null,
  imageInputAvailable: true,
  onAddImages: vi.fn(),
};

async function openOptions() {
  fireEvent.click(screen.getByRole("button", { name: "Composer options" }));
  await screen.findByRole("menu", { name: "Composer options menu" });
}

it("offers the + menu's Goal only once the Goal control is there to open", async () => {
  mutableGoalChat();
  render(<ChatComposerRegion {...props} />);
  await openOptions();
  // The Goal chunk is still loading, so there are no details to open.
  expect(screen.getByRole("menuitem", { name: "Add images" })).toBeVisible();
  expect(screen.queryByRole("menuitem", { name: "Goal" })).toBeNull();
  fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });

  await act(async () => chunk.release());
  expect(await screen.findByLabelText("Goal details")).toBeVisible();
  await openOptions();
  fireEvent.click(screen.getByRole("menuitem", { name: "Goal" }));
  expect(screen.getByLabelText("Goal details")).toHaveFocus();
});
