import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type { ComponentProps } from "react";

import type { AcpChatPage } from "../../../domain/acpProjection";
import { safeSessionNotices } from "../../../services/acpSessionNotices";
import { ChatComposerRegion } from "../ChatComposerRegion";
import { SessionNoticeStrip } from "./SessionNoticeStrip";
import { UsageLimitBanner } from "./SessionNoticeStripUsageBanner";

afterEach(cleanup);

// The session notices as the native projection serializes them.
const wire = (level: string, usedPercent: number) => [
  {
    identity: "usage-limit:seven_day",
    level,
    diagnostic: false,
    text: "You're close to your Claude weekly limit (85% used).",
    source: "claude code",
    usageLimit: {
      provider: "Claude",
      usedPercent,
      windowMinutes: 10_080,
      resetsAt: 1_795_600_000,
    },
  },
  {
    identity: "mcp-start:calendar",
    level: "warning",
    text: "Sign in to calendar.",
    source: "runtime",
  },
];

const pageWith = (notices: unknown[]) =>
  ({
    chatId: "chat-a",
    currentSessionId: "session-a",
    sessionNotices: safeSessionNotices(notices),
  }) as unknown as AcpChatPage;

const weekly = /^85% of your weekly Claude limit used · resets /;

it("shows the limit above the composer field and not in the transcript strip", async () => {
  const page = pageWith(wire("warning", 85));
  render(
    <ChatComposerRegion
      {...(composerProps as ComponentProps<typeof ChatComposerRegion>)}
      chatAuth={{ page, errorKind: null }}
    />,
  );
  const banner = (await screen.findByText(weekly)).closest("li")!;
  expect(banner).toHaveAttribute("role", "status");
  const field = screen.getByRole("combobox", { name: "Message agent" });
  expect(banner.closest("form")).toBe(field.closest("form"));
  expect(
    banner.compareDocumentPosition(field) & Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();

  cleanup();
  render(<SessionNoticeStrip session="s" notices={page.sessionNotices!} />);
  const strip = screen.getByRole("region", { name: "Session notices" });
  expect(within(strip).getByText("Sign in to calendar.")).toBeVisible();
  expect(within(strip).queryByText(/Claude weekly limit/)).toBeNull();
});

it("stays dismissed until the window's level changes, and a reached limit is an alert", () => {
  const { rerender } = render(
    <UsageLimitBanner page={pageWith(wire("warning", 85))} />,
  );
  fireEvent.click(screen.getByRole("button", { name: /^Dismiss: 85% of/ }));
  expect(screen.queryByRole("status")).toBeNull();
  rerender(<UsageLimitBanner page={pageWith(wire("warning", 92))} />);
  expect(screen.queryByRole("status")).toBeNull();
  rerender(<UsageLimitBanner page={pageWith(wire("error", 100))} />);
  expect(screen.getByRole("alert")).toHaveTextContent(
    /^You've reached your weekly Claude limit · resets /,
  );
  rerender(<UsageLimitBanner page={pageWith(wire("info", 3))} />);
  expect(screen.queryByRole("list", { name: "Usage limits" })).toBeNull();
});

const composerProps = {
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
  onSubmit: vi.fn(),
  onTextChange: vi.fn(),
  onRemoveContext: vi.fn(),
  onCancelTurn: null,
};
