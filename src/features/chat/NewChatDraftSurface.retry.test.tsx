import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../../app/store";
import type { HarnessDefinition } from "../../domain/harness";
import { desktop } from "../../services/desktop";
import { NewChatDraftSurface } from "./NewChatDraftSurface";

const initial = useWorkbenchStore.getState();
const claude: HarnessDefinition = {
  id: "claude-code",
  name: "Claude Code",
  description: "",
  source: "builtIn",
  transport: "acp",
  installState: "available",
  bindingId: "claude-code-sdk",
  accountModelDiscovery: false,
};
const unlisted =
  "Claude Code did not list its models in time. Retry in a moment.";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  useWorkbenchStore.setState(initial, true);
});

it("offers Retry on the new-chat draft when its harness did not list models", async () => {
  const listed = vi
    .spyOn(desktop, "listHarnessModels")
    .mockResolvedValue([{ id: "default", name: "Default", isDefault: true }]);
  useWorkbenchStore.setState({
    project: { id: "project-a", name: "A", roots: [] },
    projectGeneration: 0,
    projectTransitioning: false,
    harnesses: [claude],
    selectedHarnessId: claude.id,
    harnessDiscoveryPhase: "error",
    harnessDiscoveryError: unlisted,
    newChatDraft: {
      id: {},
      projectId: "project-a",
      text: "Reply with ok",
      harnessId: claude.id,
      modelId: null,
      access: null,
      effort: null,
      submission: { status: "released" },
      error: null,
    },
  });

  render(<NewChatDraftSurface />);

  // One visible copy, with its Retry; Send does not repeat it.
  expect(screen.getByText(unlisted)).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "Retry" }));

  await waitFor(() =>
    expect(useWorkbenchStore.getState()).toMatchObject({
      harnessDiscoveryPhase: "ready",
      harnessDiscoveryError: null,
    }),
  );
  expect(screen.queryByText(unlisted)).toBeNull();
  expect(listed).toHaveBeenCalledWith(claude.id);
});
