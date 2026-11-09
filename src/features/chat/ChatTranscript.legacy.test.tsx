import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../../app/store";
import type { AcpChatPage } from "../../domain/acp";
import { settleLazyModules } from "../../test/settleLazyModules";
import { page } from "./acp/acpProjection.fixture";
import { ChatTranscript } from "./ChatTranscript";

// Counts every import of the legacy transcript chunk.
const legacy = vi.hoisted(() => ({ imports: 0 }));
vi.mock("./ChatLegacyTranscript", () => {
  legacy.imports += 1;
  return { ChatLegacyTranscript: () => <p>legacy transcript</p> };
});

const initialState = useWorkbenchStore.getState();
afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialState, true);
});

function props(overrides: Partial<Parameters<typeof ChatTranscript>[0]>) {
  return {
    transcriptRef: { current: null },
    contentRef: { current: null },
    endRef: { current: null },
    onScroll: vi.fn(),
    projectTransitioning: false,
    project: { id: "project-1", name: "Sage" },
    useNativeAcp: true,
    page: null as AcpChatPage | null,
    focused: true,
    projectRoots: [],
    retentionIncomplete: false,
    onOpenResource: vi.fn(),
    hasEarlier: false,
    historyLoading: false,
    loadingEarlier: false,
    historyError: null as string | null,
    onLoadEarlier: vi.fn(),
    messages: [],
    developmentSimulator: false,
    showJumpToLatest: false,
    onJumpToLatest: vi.fn(),
    ...overrides,
  };
}

// The native case runs first: the module registry is shared across this
// file, so a later import would hide an earlier one.
it("shows a native chat's loading state, never the legacy transcript, before its first page", async () => {
  const view = render(<ChatTranscript {...props({ pageExpected: true })} />);
  expect(screen.getByText("Loading chat history…")).toBeVisible();
  await settleLazyModules();
  view.rerender(
    <ChatTranscript
      {...props({
        pageExpected: true,
        page: { ...page("completed"), turns: [] },
      })}
    />,
  );
  await settleLazyModules();
  expect(screen.queryByText("legacy transcript")).toBeNull();
  expect(legacy.imports).toBe(0);
});

it("loads the legacy transcript for a chat without a native session", async () => {
  render(<ChatTranscript {...props({ useNativeAcp: false })} />);
  expect(await screen.findByText("legacy transcript")).toBeVisible();
  expect(legacy.imports).toBe(1);
});
