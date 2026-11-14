import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { safeSessionNotices } from "../../../services/acpSessionNotices";
import { SessionNoticeStrip } from "./SessionNoticeStrip";

afterEach(cleanup);

// The notices Codex startup produces (#238): a config problem and a reroute.
const notices = safeSessionNotices([
  {
    level: "warning",
    text: "Codex configuration problem: unknown key\nIn config.toml, line 4",
    source: "codex config",
  },
  { level: "info", text: "Codex answered with gpt-5 mini", source: null },
  { level: "error", text: "Extension crashed", source: "pi extension" },
]);

describe("SessionNoticeStrip", () => {
  it("shows each notice with its level and source, and dismisses one or all", () => {
    render(<SessionNoticeStrip session="s1" notices={notices} />);
    const items = screen.getAllByRole("listitem");
    expect(items.map((item) => item.className)).toEqual([
      "acp-session-notice acp-session-notice--warning",
      "acp-session-notice acp-session-notice--info",
      "acp-session-notice acp-session-notice--error",
    ]);
    expect(items[0]!.textContent).toContain("Warning");
    expect(items[0]!.textContent).toContain("codex config");
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
