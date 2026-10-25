import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { page, textItem } from "./acp/acpProjection.fixture";
import { AcpChatPresentation } from "./acp/AcpChatPresentation";
import { settleLazyModules } from "../../test/settleLazyModules";
import { AnswerCopyFeedback } from "./answerCopyFeedback";
import {
  ChatTranscriptFooter,
  ChatTranscriptTurnSummary,
} from "./ChatTranscriptFooter";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

describe("CP-28 answer chrome", () => {
  it("does not schedule copy feedback after the footer unmounts", async () => {
    vi.useFakeTimers();
    let completeCopy!: () => void;
    const feedback = vi.fn();
    vi.stubGlobal("navigator", {
      clipboard: {
        writeText: () =>
          new Promise<void>((resolve) => {
            completeCopy = resolve;
          }),
      },
    });
    const { unmount } = render(
      <AnswerCopyFeedback.Provider value={feedback}>
        <ChatTranscriptFooter
          useNativeAcp
          page={page("completed")}
          messages={[]}
        />
      </AnswerCopyFeedback.Provider>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Copy" }));
    unmount();
    await act(async () => completeCopy());
    expect(feedback).not.toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(0);
  });
  it("renders a decorative stroked chevron in the Worked disclosure", () => {
    const { container } = render(
      <ChatTranscriptTurnSummary
        turn={page("completed").turns[0]}
        hasActivities
      />,
    );
    const chevron = container.querySelector("summary svg");
    expect(chevron).toHaveAttribute("aria-hidden", "true");
    expect(chevron).toHaveAttribute("fill", "none");
    expect(chevron).toHaveAttribute("stroke", "currentColor");
  });
  it.each([0, -1, Number.NaN, Number.POSITIVE_INFINITY, Number.MAX_VALUE])(
    "omits a first-group date for invalid timestamp %s",
    (createdAt) => {
      const next = page("completed");
      next.turns[0].createdAt = createdAt;
      next.turns[0].items = [];
      render(
        <AcpChatPresentation
          page={next}
          hasEarlier={false}
          loadingEarlier={false}
          historyError={null}
          projectRoots={[]}
          retentionIncomplete={false}
        />,
      );
      expect(screen.queryByRole("separator")).toBeNull();
    },
  );

  it.each([
    [0, false],
    [-1, false],
    [0, true],
  ] as const)(
    "dates the first visible prompt above its turn (%i days from today, invalid middle date: %s), without a duplicate same-day separator",
    async (dayOffset, invalidMiddle) => {
      const date = new Date();
      date.setDate(date.getDate() + dayOffset);
      date.setHours(9, 5, 0, 0);
      const next = page("completed");
      const first = next.turns[0];
      next.turns = (invalidMiddle ? [0, Number.NaN, 1] : [0, 1]).map(
        (offset, index) => ({
          ...first,
          id: `dated-turn-${index}` as typeof first.id,
          ordinal: index + 1,
          createdAt: date.getTime() + offset * 60_000,
          items: [
            {
              ...textItem(),
              id: `dated-prompt-${index}` as ReturnType<typeof textItem>["id"],
              createdAt: date.getTime() + offset * 60_000,
              payload: { role: "user", text: `Prompt ${index}` },
            },
          ],
        }),
      );
      const { container } = render(
        <AcpChatPresentation
          page={next}
          hasEarlier={false}
          loadingEarlier={false}
          historyError={null}
          projectRoots={[]}
          retentionIncomplete={false}
        />,
      );
      await settleLazyModules();
      const separators = screen.getAllByRole("separator");
      expect(separators).toHaveLength(1);
      expect(separators[0]).toHaveTextContent(
        dayOffset === 0 ? /^Today / : /^Yesterday /,
      );
      expect(separators[0].querySelector("time")).toHaveAttribute(
        "dateTime",
        date.toISOString(),
      );
      expect(container.querySelector(".acp-turn")?.previousElementSibling).toBe(
        separators[0],
      );
      expect(screen.getByText("Prompt 0").closest(".acp-turn")).not.toBeNull();
    },
  );

  it("keeps Copy and Retry icon-only with accessible names, title hints and copy icon feedback", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    const onRetry = vi.fn();
    render(
      <ChatTranscriptFooter
        useNativeAcp
        page={page("completed")}
        messages={[]}
        onRetry={onRetry}
      />,
    );
    const copy = screen.getByRole("button", { name: "Copy" });
    const retry = screen.getByRole("button", { name: "Retry" });
    for (const button of [copy, retry]) {
      expect(button.textContent).toBe("");
      expect(button).toHaveAttribute(
        "title",
        button.getAttribute("aria-label"),
      );
      expect(button.querySelector("svg")).toHaveAttribute(
        "aria-hidden",
        "true",
      );
    }
    fireEvent.click(copy);
    await waitFor(() => expect(copy).toHaveAccessibleName("Copied"));
    expect(copy).toHaveAttribute("title", "Copied");
    expect(writeText).toHaveBeenCalledExactlyOnceWith(textItem().payload.text);
    fireEvent.click(retry);
    expect(onRetry).toHaveBeenCalledOnce();
  });
});
