import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import type { AcpChatPage, TurnProjection } from "../../domain/acp";
import { AcpChatPresentation, InheritedTurns } from "./acp/AcpChatPresentation";
import { page, textItem } from "./acp/acpProjection.fixture";
import { AnswerCopyFeedback } from "./answerCopyFeedback";
import { ChatTranscriptFooter } from "./ChatTranscriptFooter";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

const css = (path: string) => readFileSync(resolve(path), "utf8");

/** Two completed Turns, each with its own answer. */
function twoTurns(): AcpChatPage {
  const projection = page("completed");
  const first = projection.turns[0];
  const second: TurnProjection = {
    ...first,
    id: "turn-2" as TurnProjection["id"],
    ordinal: 2,
    items: [
      {
        ...textItem(),
        id: "item-second" as never,
        createdAt: Date.UTC(2026, 7, 31, 9, 40),
        payload: { role: "assistant", text: "Second answer" },
      },
    ],
  };
  return { ...projection, turns: [first, second] };
}

function presentation(projection: AcpChatPage) {
  return render(
    <AcpChatPresentation
      page={projection}
      hasEarlier={false}
      loadingEarlier={false}
      historyError={null}
      projectRoots={[]}
      retentionIncomplete={false}
    />,
  );
}

const turnSection = (ordinal: number) =>
  screen.getByRole("region", { name: `Turn ${ordinal}` });

it("keeps a copy row under every earlier completed answer (Codex s2/f0149)", async () => {
  const writeText = vi.fn().mockResolvedValue(undefined);
  vi.stubGlobal("navigator", { clipboard: { writeText } });
  presentation(twoTurns());
  const row = within(turnSection(1)).getByRole("group", {
    name: "Message actions",
  });
  // The latest answer's row is the transcript footer's, not the Turn's.
  expect(
    within(turnSection(2)).queryByRole("group", { name: "Message actions" }),
  ).toBeNull();
  await act(async () => {
    fireEvent.click(within(row).getByRole("button", { name: "Copy" }));
  });
  expect(writeText).toHaveBeenCalledWith("Normalized answer");
  expect(within(row).getByRole("button", { name: "Copied" })).toBeVisible();
});

/** The menu a row's "…" opens, by its item labels. */
async function moreItems(row: HTMLElement) {
  fireEvent.click(
    await within(row).findByRole("button", { name: "More actions" }),
  );
  const menu = await screen.findByRole("menu", { name: "Answer actions" });
  const items = within(menu)
    .getAllByRole("menuitem")
    .map((item) => item.textContent);
  fireEvent.keyDown(menu, { key: "Escape" });
  return items;
}

const forkable = (projection: AcpChatPage): AcpChatPage => ({
  ...projection,
  sessions: [
    {
      id: "session-1",
      runtimeEpoch: "2",
      residency: "hot",
      capabilities: { nativeSessionFork: true },
    },
  ] as never,
});

it('puts "…" on an earlier answer, offering only Copy there (s2/f0066)', async () => {
  const writeText = vi.fn().mockResolvedValue(undefined);
  vi.stubGlobal("navigator", { clipboard: { writeText } });
  // The session can fork, but only through its latest Turn.
  presentation(forkable(twoTurns()));
  const row = within(turnSection(1)).getByRole("group", {
    name: "Message actions",
  });
  expect(await moreItems(row)).toEqual(["Copy"]);
  fireEvent.click(within(row).getByRole("button", { name: "More actions" }));
  await act(async () => {
    fireEvent.click(await screen.findByRole("menuitem", { name: "Copy" }));
  });
  expect(writeText).toHaveBeenCalledWith("Normalized answer");
});

it('keeps Copy and "…" usable on inherited fork answers, with no fork there (s2/f0066)', async () => {
  const writeText = vi.fn().mockResolvedValue(undefined);
  vi.stubGlobal("navigator", { clipboard: { writeText } });
  const { container } = render(<InheritedTurns page={forkable(twoTurns())} />);
  const rows = screen.getAllByRole("group", { name: "Message actions" });
  expect(rows).toHaveLength(2);
  rows.forEach((row, index) => {
    const turn = row.closest("section");
    expect(turn).not.toBeNull();
    expect(turn).toHaveAccessibleName(`Turn ${index + 1}`);
    expect(turn).toContainElement(
      container.querySelectorAll<HTMLElement>(".acp-turn")[index],
    );
  });
  // The Turn is inert; its answer's own row is not inside it.
  for (const turn of container.querySelectorAll(".acp-turn"))
    expect(turn).toHaveAttribute("inert");
  for (const row of rows) expect(row.closest("[inert]")).toBeNull();
  expect(await moreItems(rows[1])).toEqual(["Copy"]);
  await act(async () => {
    fireEvent.click(within(rows[1]).getByRole("button", { name: "Copy" }));
  });
  expect(writeText).toHaveBeenCalledWith("Second answer");
});

it("announces Copy on an inherited answer through the transcript's feedback", async () => {
  const writeText = vi.fn().mockResolvedValue(undefined);
  vi.stubGlobal("navigator", { clipboard: { writeText } });
  const announce = vi.fn();
  render(
    <AnswerCopyFeedback.Provider value={announce}>
      <InheritedTurns page={twoTurns()} />
    </AnswerCopyFeedback.Provider>,
  );
  const rows = screen.getAllByRole("group", { name: "Message actions" });
  await act(async () => {
    fireEvent.click(within(rows[0]).getByRole("button", { name: "Copy" }));
  });
  expect(announce).toHaveBeenCalledExactlyOnceWith("Copied");
});

it("shows the answer's own time in the row, and none without a real time (s2/f0093)", () => {
  const projection = twoTurns();
  presentation(projection);
  const time = within(turnSection(1)).getByText(
    new Intl.DateTimeFormat(undefined, {
      hour: "numeric",
      minute: "2-digit",
    }).format(Date.UTC(2026, 7, 31, 9, 5)),
  );
  expect(time.tagName).toBe("TIME");
  expect(time).toHaveAttribute(
    "datetime",
    new Date(Date.UTC(2026, 7, 31, 9, 5)).toISOString(),
  );
  expect(time).toHaveClass("chat-transcript-actions__time");
  cleanup();

  const timed = twoTurns();
  const untimed: AcpChatPage = {
    ...timed,
    turns: [
      { ...timed.turns[0], items: [{ ...textItem(), createdAt: 0 }] },
      timed.turns[1],
    ],
  };
  presentation(untimed);
  expect(
    within(turnSection(1))
      .getByRole("group", { name: "Message actions" })
      .querySelector("time"),
  ).toBeNull();
});

it("gives the latest answer's footer the same timed row", () => {
  render(
    <ChatTranscriptFooter
      useNativeAcp
      page={page("completed")}
      messages={[]}
    />,
  );
  const row = screen.getByRole("group", { name: "Message actions" });
  expect(
    row.querySelector("time.chat-transcript-actions__time"),
  ).toHaveAttribute(
    "datetime",
    new Date(Date.UTC(2026, 7, 31, 9, 5)).toISOString(),
  );
});

it("pitches action buttons 24 px apart, fills the open menu trigger and reveals the time on hover (s2/f0053)", () => {
  const transcript = css("src/features/chat/chat-transcript-polish.css");
  expect(transcript).toMatch(/\.chat-transcript-actions\s*\{[^}]*gap:\s*0;/s);
  expect(transcript).toMatch(
    /\.chat-transcript-actions > button\s*\{[^}]*width:\s*24px;[^}]*height:\s*24px;[^}]*padding:\s*0;/s,
  );
  expect(transcript).toMatch(
    /\.chat-transcript-actions > button\[aria-expanded="true"\]\s*\{[^}]*background:\s*var\(--color-surface-hover\);/s,
  );
  expect(transcript).toMatch(
    /\.chat-transcript-actions__time\s*\{[^}]*color:\s*var\(--color-text-muted\);[^}]*opacity:\s*0;/s,
  );
  expect(transcript).toMatch(
    /\.chat-transcript-actions:hover > \.chat-transcript-actions__time,\s*\.chat-transcript-actions:focus-within > \.chat-transcript-actions__time\s*\{\s*opacity:\s*1;/s,
  );
  // Hovering the answer, not only its row, shows the time (s2/f0093).
  expect(transcript).toMatch(
    /\.acp-turn:hover \.chat-transcript-actions__time,\s*\.acp-turn:hover \+ \.chat-transcript-actions > \.chat-transcript-actions__time,\s*\.message-list__content:has\(\s*> \.acp-chat-projection > \.acp-turn:last-of-type:hover\s*\)\s*> \.chat-transcript-actions\s*> \.chat-transcript-actions__time\s*\{\s*opacity:\s*1;/s,
  );
  // The open "…" dots and its menu label are white (s2/f0053).
  expect(transcript).toMatch(
    /\.chat-transcript-actions > button\[aria-expanded="true"\]\s*\{\s*color:\s*var\(--color-text-emphasis\);/s,
  );
  expect(css("src/features/chat/fork-chat.css")).toMatch(
    /\.answer-more-menu__item,\s*\.fork-chat-dialog button\s*\{[^}]*color:\s*var\(--color-status-foreground\);/s,
  );
  // A tight icon-to-label gap in the "Branch in new chat" item.
  expect(css("src/features/chat/ForkChat.tsx")).toContain(
    'className="button--compact answer-more-menu__item"',
  );
  expect(css("src/styles/shared-controls.css")).toMatch(
    /\.button--compact\s*\{[^}]*gap:\s*4px;/s,
  );
});
