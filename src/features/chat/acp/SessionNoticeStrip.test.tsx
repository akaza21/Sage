import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { safeSessionNotices } from "../../../services/acpSessionNotices";
import { SessionNoticeStrip } from "./SessionNoticeStrip";

afterEach(cleanup);

const notices = safeSessionNotices([
  {
    level: "warning",
    text: "Approval review needs attention",
    source: "fixture",
    diagnostic: false,
  },
  {
    level: "info",
    text: "Codex answered with gpt-5 mini",
    source: null,
    diagnostic: false,
  },
  { level: "error", text: "Extension crashed", source: "pi extension" },
]);

describe("SessionNoticeStrip", () => {
  it.each([
    ["error", "Tools unavailable."],
    ["warning", "Sign in to restore tools."],
  ] as const)(
    "resurfaces a dismissed problem updated to %s: %s",
    (level, text) => {
      const original = {
        identity: "access-mode-guard",
        level: "warning" as const,
        text: "Tools unavailable.",
        source: "runtime",
      };
      const { rerender } = render(
        <SessionNoticeStrip session="s1" notices={[original]} />,
      );
      fireEvent.click(screen.getByRole("button", { name: /Dismiss warning/ }));
      rerender(<SessionNoticeStrip session="s1" notices={[{ ...original }]} />);
      expect(screen.queryByRole("listitem")).toBeNull();
      rerender(
        <SessionNoticeStrip
          session="s1"
          notices={[{ ...original, level, text }]}
        />,
      );
      expect(screen.getByText(text)).toBeVisible();
      expect(screen.queryByRole("button", { name: "Dismiss all" })).toBeNull();
    },
  );
  it("shows one notice per problem identity, the latest, across wording and sources", () => {
    const decoded = safeSessionNotices([
      {
        identity: "mcp-start:calendar",
        level: "warning",
        text: "Calendar tools are unavailable.",
        source: "runtime",
      },
      {
        identity: "mcp-start:calendar",
        level: "error",
        text: "Calendar needs sign-in.",
        source: "adapter",
      },
    ]);
    render(<SessionNoticeStrip session="s1" notices={decoded} />);
    expect(screen.getAllByRole("listitem")).toHaveLength(1);
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Calendar needs sign-in.",
    );
    expect(screen.queryByText("Calendar tools are unavailable.")).toBeNull();
  });

  it("keeps different problems separate even with identical text", () => {
    render(
      <SessionNoticeStrip
        session="s1"
        notices={safeSessionNotices([
          {
            identity: "mcp-start:calendar",
            level: "warning",
            text: "Tools unavailable.",
            source: "runtime",
          },
          {
            identity: "mcp-start:mail",
            level: "warning",
            text: "Tools unavailable.",
            source: "runtime",
          },
        ])}
      />,
    );
    expect(screen.getAllByRole("listitem")).toHaveLength(2);
    expect(screen.getAllByText("Tools unavailable.")).toHaveLength(2);
  });
  it("replays legacy actionable warnings visibly in chat", () => {
    const legacy = safeSessionNotices([
      {
        level: "warning",
        text: "The MCP server calendar needs you to sign in again; its tools are unavailable until then.",
        source: "codex mcp",
      },
      { level: "warning", text: "Sandbox is degraded.", source: "codex" },
      { level: "error", text: "Legacy blocking failure", source: null },
    ]);
    render(<SessionNoticeStrip session="restored" notices={legacy} />);
    expect(
      screen.getByText(
        "The MCP server calendar needs you to sign in again; its tools are unavailable until then.",
      ),
    ).toBeVisible();
    expect(screen.getByText("Sandbox is degraded.")).toBeVisible();
    expect(screen.getByText("Legacy blocking failure")).toBeVisible();
  });
  it("remounting a chat replays saved warnings silently but still interrupts for errors", () => {
    const { unmount } = render(
      <SessionNoticeStrip session="s1" notices={notices} />,
    );
    unmount();
    render(<SessionNoticeStrip session="s1" notices={notices} />);
    const warning = screen
      .getByText("Approval review needs attention")
      .closest(".notice");
    expect(warning).toHaveAttribute("aria-live", "off");
    expect(screen.getByRole("alert")).toHaveTextContent("Extension crashed");
  });
  it("keeps protocol diagnostics out of chat while retaining blocking errors", () => {
    const decoded = safeSessionNotices([
      {
        level: "warning",
        text: "Unknown configuration key",
        source: "fixture",
        diagnostic: true,
      },
      {
        level: "error",
        text: "Session cannot start",
        source: "fixture",
        diagnostic: true,
      },
      {
        level: "warning",
        text: "Approval requires attention",
        source: "fixture",
        diagnostic: false,
      },
    ]);
    render(<SessionNoticeStrip session="s1" notices={decoded} />);
    expect(screen.queryByText("Unknown configuration key")).toBeNull();
    expect(screen.getByText("Session cannot start")).toBeVisible();
    expect(screen.getByText("Approval requires attention")).toBeVisible();
  });
  it("shows each notice with its level and source, and dismisses one or all", () => {
    render(<SessionNoticeStrip session="s1" notices={notices} />);
    const items = screen.getAllByRole("listitem");
    expect(
      items.map(
        (item) => item.querySelector("[data-notice-identity]")?.className,
      ),
    ).toEqual([
      "notice notice--warning",
      "notice notice--info",
      "notice notice--blocking",
    ]);
    expect(items[0]!.textContent).toContain("Warning");
    fireEvent.click(within(items[0]).getByRole("button", { name: "Details" }));
    expect(items[0]!.textContent).toContain("fixture");
    expect(items[2]!.textContent).toContain("Extension crashed");

    fireEvent.click(
      screen.getByRole("button", {
        name: "Dismiss notice: Codex answered with gpt-5 mini",
      }),
    );
    expect(screen.getAllByRole("listitem")).toHaveLength(2);

    fireEvent.click(screen.getByRole("button", { name: "Dismiss all" }));
    expect(screen.queryByRole("region", { name: "Session notices" })).toBe(
      null,
    );
  });

  it("keeps dismissals per session and only for notices still listed", () => {
    const { rerender } = render(
      <SessionNoticeStrip session="s1" notices={notices} />,
    );
    const dismissReroute = () =>
      fireEvent.click(
        screen.getByRole("button", {
          name: "Dismiss notice: Codex answered with gpt-5 mini",
        }),
      );
    dismissReroute();
    expect(screen.getAllByRole("listitem")).toHaveLength(2);

    rerender(<SessionNoticeStrip session="s2" notices={notices} />);
    expect(screen.getAllByRole("listitem")).toHaveLength(3);

    dismissReroute();
    rerender(
      <SessionNoticeStrip
        session="s2"
        notices={notices.filter((notice) => notice.level !== "info")}
      />,
    );
    rerender(<SessionNoticeStrip session="s2" notices={notices} />);
    expect(screen.getAllByRole("listitem")).toHaveLength(3);
  });

  it("decodes only well-formed notices and keeps the newest sixteen", () => {
    const many = Array.from({ length: 20 }, (_, index) => ({
      level: "info",
      text: `notice ${index}`,
      source: null,
    }));
    const decoded = safeSessionNotices([
      ...many,
      { level: "loud", text: "x", source: null },
    ]);
    expect(decoded).toHaveLength(15);
    expect(decoded[0]!.text).toBe("notice 5");
    expect(safeSessionNotices("nope")).toEqual([]);
    expect(
      safeSessionNotices([{ level: "info", text: "", source: null }]),
    ).toEqual([]);
  });
});
