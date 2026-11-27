import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../../app/store";
import type { HarnessDefinition } from "../../domain/harness";
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
  useWorkbenchStore.setState(initial, true);
});

it("offers Retry on the new-chat draft when its harness did not list models", () => {
  const retryHarnessDiscovery = vi.fn(async () => {});
  useWorkbenchStore.setState({
    project: { id: "project-a", name: "A", roots: [] },
    harnesses: [claude],
    selectedHarnessId: claude.id,
    harnessDiscoveryPhase: "error",
    harnessDiscoveryError: unlisted,
    retryHarnessDiscovery,
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

  expect(screen.getAllByText(unlisted).length).toBeGreaterThan(0);
  fireEvent.click(screen.getByRole("button", { name: "Retry" }));
  expect(retryHarnessDiscovery).toHaveBeenCalledOnce();
});
