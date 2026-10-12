import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { AcpChatPage, AcpItem, TurnState } from "../../../domain/acp";
import { AcpChatPresentation } from "./AcpChatPresentation";
import { page, textItem } from "./acpProjection.fixture";
import { settleLazyModules } from "../../../test/settleLazyModules";

afterEach(cleanup);

function retryPage(state: TurnState): AcpChatPage {
  const next = page(
    state,
    state === "failed"
      ? { kind: "other", message: "Retries exhausted" }
      : undefined,
  );
  next.turns = [
    {
      ...next.turns[0],
      createdAt: 1,
      terminalAt: state === "running" ? null : 5001,
      items: [
        {
          ...textItem(),
          id: "failed-attempt" as AcpItem["id"],
          ordinal: 0,
          lifecycle: "failed",
          payload: { role: "assistant", text: "First attempt retained" },
        },
        {
          ...textItem(),
          id: "attempt-cause" as AcpItem["id"],
          ordinal: 1,
          lifecycle: "failed",
          kind: "error",
          payload: {
            kind: "attempt",
            code: "Attempt failed",
            message: "Server unavailable",
            recoverable: false,
          },
        } as AcpItem,
        ...(state === "completed" || state === "failed"
          ? [
              {
                ...textItem(),
                id: "last-attempt" as AcpItem["id"],
                ordinal: 2,
                lifecycle:
                  state === "completed"
                    ? ("completed" as const)
                    : ("failed" as const),
                payload: {
                  role: "assistant" as const,
                  text: "Last attempt retained",
                },
              },
            ]
          : []),
      ],
    },
  ];
  return next;
}

function presentation(next: AcpChatPage, onRetry = vi.fn()) {
  return (
    <AcpChatPresentation
      page={next}
      projectRoots={[]}
      hasEarlier={false}
      loadingEarlier={false}
      historyError={null}
      retentionIncomplete={false}
      onRetry={onRetry}
    />
  );
}

describe.each(["codex", "claude-code", "pi", "opencode", "oh-my-pi"])(
  "%s shared normalized projection",
  (bindingId) => {
    it.each(["running", "completed", "failed", "interrupted"] as const)(
      "retains emitted attempt text and cause in one %s turn",
      async (state) => {
        const next = retryPage(state);
        next.sessions = next.sessions.map((session) => ({
          ...session,
          bindingId: bindingId as AcpChatPage["sessions"][number]["bindingId"],
        }));
        const onRetry = vi.fn();
        const { container } = render(presentation(next, onRetry));
        await settleLazyModules();
        expect(container.querySelectorAll(".acp-turn")).toHaveLength(1);
        const work = container.querySelector(".chat-transcript-work")!;
        if (state !== "running")
          fireEvent.click(
            within(work as HTMLElement).getByText("Worked for 5s"),
          );
        expect(screen.getByText("First attempt retained")).toBeVisible();
        expect(
          within(work as HTMLElement).getByText("Server unavailable"),
        ).toBeVisible();
        if (state === "completed" || state === "failed") {
          expect(
            screen
              .getByText("First attempt retained")
              .closest(".chat-transcript-work"),
          ).toBe(work);
          expect(
            screen
              .getByText("Last attempt retained")
              .closest(".chat-transcript-work"),
          ).toBeNull();
        }
        if (state === "failed") {
          const failure = screen.getByRole("group", { name: "Turn failed" });
          expect(within(failure).getByText("Retries exhausted")).toBeVisible();
          expect(within(failure).queryByText("Server unavailable")).toBeNull();
          fireEvent.click(
            within(failure).getByRole("button", { name: "Retry" }),
          );
          expect(onRetry).toHaveBeenCalledOnce();
        } else {
          expect(
            screen.queryByRole("group", { name: "Turn failed" }),
          ).toBeNull();
          expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
        }
      },
    );

    it("shows no failure or image when the harness emits neither", () => {
      const next = page("running");
      next.sessions = next.sessions.map((session) => ({
        ...session,
        bindingId: bindingId as AcpChatPage["sessions"][number]["bindingId"],
      }));
      next.turns[0].items = [];
      render(presentation(next));
      expect(screen.queryByRole("img")).toBeNull();
      expect(screen.queryByText("Attempt failed")).toBeNull();
      expect(screen.queryByText("Reason unavailable")).toBeNull();
      expect(screen.queryByRole("group", { name: "Turn failed" })).toBeNull();
    });
  },
);

it("opens a durable Pi image in only its originating chat pane", async () => {
  const next = page("completed");
  next.sessions = next.sessions.map((session) => ({
    ...session,
    bindingId: "pi" as AcpChatPage["sessions"][number]["bindingId"],
  }));
  next.turns = [
    {
      ...next.turns[0],
      createdAt: 1,
      terminalAt: 5001,
      items: [
        {
          ...textItem(),
          kind: "image",
          lifecycle: "completed",
          ordinal: 0,
          payload: {
            mediaType: "image/png",
            source: "agent",
            dataUrl:
              "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/xioAAAAASUVORK5CYII=",
          },
        } as AcpItem,
      ],
    },
  ];
  render(
    <>
      <section aria-label="Chat one">{presentation(next)}</section>
      <section aria-label="Chat two">{presentation(next)}</section>
    </>,
  );
  await settleLazyModules();
  const first = within(screen.getByRole("region", { name: "Chat one" }));
  const second = within(screen.getByRole("region", { name: "Chat two" }));
  fireEvent.click(first.getByText("Worked for 5s"));
  fireEvent.click(second.getByText("Worked for 5s"));
  expect(first.getByRole("img")).toHaveAttribute(
    "src",
    next.turns[0].items[0].kind === "image"
      ? next.turns[0].items[0].payload.dataUrl
      : "",
  );
  expect(second.getByRole("img")).toBeVisible();
  const firstTrigger = first.getByRole("button", { name: "Open image viewer" });
  const secondTrigger = second.getByRole("button", {
    name: "Open image viewer",
  });
  fireEvent.click(firstTrigger);
  expect(screen.getAllByRole("dialog")).toHaveLength(1);
  expect(screen.getByRole("dialog")).toBeVisible();
  expect(second.queryByRole("dialog")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Close image viewer" }));
  await waitFor(() => expect(firstTrigger).toHaveFocus());
  expect(screen.queryByRole("dialog")).toBeNull();
  fireEvent.click(secondTrigger);
  expect(screen.getAllByRole("dialog")).toHaveLength(1);
  fireEvent.click(screen.getByRole("button", { name: "Close image viewer" }));
  await waitFor(() => expect(secondTrigger).toHaveFocus());
  expect(screen.queryByText("Pi image output is not yet admitted")).toBeNull();
});

it("keeps an earlier automatic attempt cause under Worked when the final cause is absent", async () => {
  const next = retryPage("failed");
  next.turns[0].terminalReason = { kind: "other" };
  render(presentation(next));
  await settleLazyModules();
  fireEvent.click(screen.getByText("Worked for 5s"));
  expect(screen.getByText("Server unavailable")).toBeVisible();
  const failure = screen.getByRole("group", { name: "Turn failed" });
  expect(within(failure).getByText("Reason unavailable")).toBeVisible();
  expect(within(failure).queryByText("Server unavailable")).toBeNull();
});
