import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import { acpProjectionStoreFor } from "../../app/acpProjectionRegistry";
import { snapshotPage } from "../../app/agentDiff.fixture";
import type { AcpChatPage, TurnProjection } from "../../domain/acp";
import { AcpChatPresentation } from "./acp/AcpChatPresentation";
import { InheritedTurns } from "./ForkHistory";
import { page, textItem } from "./acp/acpProjection.fixture";
import { AnswerCopyFeedback } from "./answerCopyFeedback";
import { ConversationRequestContext } from "./forkChatRequest";

afterEach(() => {
  cleanup();
  acpProjectionStoreFor("chat-1").setState({ page: null });
  vi.unstubAllGlobals();
});

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

it("keeps a copy row under every completed answer, the latest included (Codex s2/f0149)", async () => {
  const writeText = vi.fn().mockResolvedValue(undefined);
  vi.stubGlobal("navigator", { clipboard: { writeText } });
  presentation(twoTurns());
  const row = within(turnSection(1)).getByRole("group", {
    name: "Message actions",
  });
  expect(
    within(turnSection(2)).getByRole("group", { name: "Message actions" }),
  ).toBeVisible();
  await act(async () => {
    fireEvent.click(within(row).getByRole("button", { name: "Copy" }));
  });
  expect(writeText).toHaveBeenCalledWith("Normalized answer");
  expect(within(row).getByRole("button", { name: "Copied" })).toBeVisible();
});

it("closes a Turn with its action row, after its edited files", async () => {
  const projection = snapshotPage();
  const [edit] = projection.turns[0].items;
  const answer = { ...textItem(), ordinal: 2 };
  const latest = twoTurns().turns[1];
  projection.turns = [
    { ...projection.turns[0], items: [edit, answer] },
    { ...latest, sessionId: projection.turns[0].sessionId },
  ];
  presentation(projection);
  const turn = turnSection(1);
  const files = await within(turn).findByRole("region", {
    name: "Edited files",
  });
  const row = within(turn).getByRole("group", { name: "Message actions" });
  expect(
    files.compareDocumentPosition(row) & Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();
  expect(turn.lastElementChild).toBe(row);
});

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
      capabilities: {
        rewind: {
          effects: { conversation: false, files: false },
          targets: "latestTurn",
          forkAt: true,
          onResume: false,
        },
      },
    },
  ] as never,
});

it('puts "…" on an earlier answer, offering only Copy there (s2/f0066)', async () => {
  const writeText = vi.fn().mockResolvedValue(undefined);
  vi.stubGlobal("navigator", { clipboard: { writeText } });
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
  const rows = await screen.findAllByRole("group", { name: "Message actions" });
  expect(rows).toHaveLength(2);
  rows.forEach((row, index) => {
    const turn = row.closest("section");
    expect(turn).not.toBeNull();
    expect(turn).toHaveAccessibleName(`Turn ${index + 1}`);
    expect(turn).toContainElement(
      container.querySelectorAll<HTMLElement>(".acp-turn")[index],
    );
  });
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
  const rows = await screen.findAllByRole("group", { name: "Message actions" });
  await act(async () => {
    fireEvent.click(within(rows[0]).getByRole("button", { name: "Copy" }));
  });
  expect(announce).toHaveBeenCalledExactlyOnceWith("Copied");
});

it("shows the answer's own time in the row, and none without a real time (s2/f0093)", () => {
  const projection = twoTurns();
  presentation(projection);
  const time = within(
    within(turnSection(1)).getByRole("group", { name: "Message actions" }),
  ).getByText(
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

it("retains the owner-requested Retry on a successful native answer", () => {
  const onRetry = vi.fn();
  const projection = page("completed");
  const ui = (retry?: () => void) => (
    <AcpChatPresentation
      page={projection}
      hasEarlier={false}
      loadingEarlier={false}
      historyError={null}
      projectRoots={[]}
      retentionIncomplete={false}
      onRetry={retry}
    />
  );
  const { rerender } = render(ui(onRetry));
  const row = screen.getByRole("group", { name: "Message actions" });
  fireEvent.click(within(row).getByRole("button", { name: "Retry" }));
  expect(onRetry).toHaveBeenCalledOnce();
  expect(within(row).getByRole("button", { name: "Copy" })).toBeVisible();
  rerender(ui());
  expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
});

it("offers Retry and Branch only on the latest answer, following its session's residency", async () => {
  const projection = forkable(twoTurns());
  const ui = (next: AcpChatPage) => (
    <ConversationRequestContext.Provider value={vi.fn()}>
      <AcpChatPresentation
        page={next}
        hasEarlier={false}
        loadingEarlier={false}
        historyError={null}
        projectRoots={[]}
        retentionIncomplete={false}
        onRetry={vi.fn()}
      />
    </ConversationRequestContext.Provider>
  );
  const { rerender } = render(ui(projection));
  const row = (ordinal: number) =>
    within(turnSection(ordinal)).getByRole("group", {
      name: "Message actions",
    });
  expect(within(row(1)).queryByRole("button", { name: "Retry" })).toBeNull();
  expect(within(row(2)).getByRole("button", { name: "Retry" })).toBeVisible();
  expect(await moreItems(row(1))).toEqual(["Copy"]);
  expect(await moreItems(row(2))).toEqual(["Branch in new chat"]);

  const parked: AcpChatPage = {
    ...projection,
    sessions: projection.sessions.map((session) => ({
      ...session,
      residency: "cold",
    })) as never,
  };
  await act(async () => {
    acpProjectionStoreFor(projection.chatId).setState({ page: parked });
  });
  rerender(ui(parked));
  expect(await moreItems(row(2))).toEqual(["Copy"]);
});

it("gives the latest answer the same timed row", () => {
  presentation(page("completed"));
  const row = screen.getByRole("group", { name: "Message actions" });
  expect(
    row.querySelector("time.chat-transcript-actions__time"),
  ).toHaveAttribute(
    "datetime",
    new Date(Date.UTC(2026, 7, 31, 9, 5)).toISOString(),
  );
});
