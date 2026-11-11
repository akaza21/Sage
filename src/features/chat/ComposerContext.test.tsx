import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { AcpUsageRequest } from "../../domain/acpUsage";
import { acpUsageDesktop } from "../../services/acpUsageDesktop";
import type { ChatUsage } from "./acp/chatUsage";
import { ComposerContext } from "./ComposerContext";

afterEach(cleanup);

function usage(totalTokens: number, contextWindow?: number): ChatUsage {
  return {
    inputTokens: totalTokens,
    outputTokens: 0,
    totalTokens,
    ...(contextWindow === undefined ? {} : { contextWindow }),
  };
}

describe("ComposerContext", () => {
  it("hides context when capacity is unknown", () => {
    render(<ComposerContext usage={usage(6_000)} />);
    expect(screen.queryByLabelText("Composer context")).toBeNull();
    expect(screen.queryByRole("progressbar")).toBeNull();
  });

  it("renders reported usage against a known runtime context window", async () => {
    render(<ComposerContext usage={usage(6_000, 272_000)} />);
    await screen.findByRole("progressbar");

    expect(screen.getByLabelText("Composer context")).toHaveTextContent(
      "6K context",
    );
    expect(
      document.querySelector(".composer__context-usage-label"),
    ).toHaveTextContent("6K context");
    const meter = screen.getByRole("progressbar", {
      name: "6,000 used / 272,000 total tokens",
    });
    expect(meter).toHaveAttribute("aria-valuemin", "0");
    expect(meter).toHaveAttribute("aria-valuemax", "272000");
    expect(meter).toHaveAttribute("aria-valuenow", "6000");
    expect(meter).toHaveAttribute(
      "aria-valuetext",
      `${(6_000).toLocaleString()} / ${(272_000).toLocaleString()} tokens used`,
    );
    expect(meter).toHaveAttribute("title", "6k / 272k tokens · 2%");
    expect(meter).toHaveAttribute("tabindex", "0");
    expect(meter).toHaveAttribute(
      "aria-label",
      "6,000 used / 272,000 total tokens",
    );
    expect(meter).not.toHaveTextContent("%");
  });

  it("clamps only the visual and numeric progressbar value when usage is over capacity", async () => {
    render(<ComposerContext usage={usage(300_000, 272_000)} />);
    await screen.findByRole("progressbar");

    expect(screen.getByLabelText("Composer context")).toHaveTextContent(
      "300K context",
    );
    expect(
      document.querySelector(".composer__context-usage-label"),
    ).toHaveTextContent("300K context");
    const meter = screen.getByRole("progressbar", {
      name: "300,000 used / 272,000 total tokens",
    });
    expect(meter).toHaveAttribute("aria-valuenow", "272000");
    expect(meter).toHaveAttribute(
      "aria-valuetext",
      `${(300_000).toLocaleString()} / ${(272_000).toLocaleString()} tokens used`,
    );
    expect(meter).toHaveAttribute("title", "300k / 272k tokens · 110%");
    expect(meter).toHaveStyle({ "--composer-context-usage": "100%" });
  });

  it("does not render a meter for invalid in-memory capacity", () => {
    render(<ComposerContext usage={usage(6_000, 0)} />);

    expect(screen.queryByLabelText("Composer context")).toBeNull();
    expect(screen.queryByRole("progressbar")).toBeNull();
  });

  it("does not render usage or a meter without a reported total", () => {
    const { rerender } = render(<ComposerContext usage={null} />);

    expect(screen.queryByText(/Reported usage/)).toBeNull();
    expect(screen.queryByRole("progressbar")).toBeNull();

    rerender(<ComposerContext usage={null} />);
    expect(screen.queryByText(/Reported usage/)).toBeNull();
    expect(screen.queryByRole("progressbar")).toBeNull();
  });

  it("reads usage limits only when opened and drops them when closed", async () => {
    const read = vi.spyOn(acpUsageDesktop, "read").mockResolvedValue({
      kind: "usage",
      snapshot: {
        plan: "Pro",
        limitReached: true,
        windows: [
          {
            label: null,
            usedPercent: 42,
            windowMinutes: 300,
            resetsAt: null,
          },
        ],
        session: null,
      },
    });
    const request = {
      projectId: "project-a",
      projectGeneration: "1",
      chatId: "chat-a",
      sessionId: "session-a",
      runtimeEpoch: "2",
    } as AcpUsageRequest;
    render(
      <ComposerContext usage={usage(6_000, 272_000)} usageRequest={request} />,
    );
    const trigger = await screen.findByRole("button", {
      name: /Show usage/,
    });
    expect(read).not.toHaveBeenCalled();

    fireEvent.click(trigger);
    expect(
      await screen.findByRole("progressbar", { name: "5-hour used" }),
    ).toHaveAttribute("aria-valuenow", "42");
    expect(screen.getByRole("alert")).toHaveTextContent("Usage limit reached.");
    expect(screen.getByText("Pro")).toBeVisible();
    expect(read).toHaveBeenCalledWith(request);

    fireEvent.click(trigger);
    expect(screen.queryByRole("dialog", { name: "Usage" })).toBeNull();
    read.mockRestore();
  });

  it("shows a session's tokens and cost when its harness reports no limits", async () => {
    const read = vi.spyOn(acpUsageDesktop, "read").mockResolvedValue({
      kind: "usage",
      snapshot: {
        plan: null,
        limitReached: false,
        windows: [],
        session: {
          inputTokens: 50_000,
          outputTokens: 10_000,
          cacheReadTokens: 40_000,
          cacheWriteTokens: 0,
          totalTokens: 100_000,
          costUsd: 0.0125,
        },
      },
    });
    render(
      <ComposerContext
        usage={usage(6_000, 272_000)}
        usageRequest={
          {
            projectId: "project-a",
            projectGeneration: "1",
            chatId: "chat-a",
            sessionId: "session-a",
            runtimeEpoch: "2",
          } as AcpUsageRequest
        }
      />,
    );
    fireEvent.click(await screen.findByRole("button", { name: /Show usage/ }));
    const rows = await screen.findByLabelText("This session");
    const pairs = [...rows.querySelectorAll("div")].map((row) => [
      row.querySelector("dt")?.textContent,
      row.querySelector("dd")?.textContent,
    ]);
    expect(pairs).toEqual([
      ["Input", (50_000).toLocaleString()],
      ["Output", (10_000).toLocaleString()],
      ["Cache read", (40_000).toLocaleString()],
      ["Total tokens", (100_000).toLocaleString()],
      ["Cost", expect.stringMatching(/0[.,]0125/)],
    ]);
    expect(
      screen.getByText("This session", { selector: "strong" }),
    ).toBeVisible();
    expect(screen.queryByRole("progressbar", { name: /used$/ })).toBeNull();
    read.mockRestore();
  });
});
