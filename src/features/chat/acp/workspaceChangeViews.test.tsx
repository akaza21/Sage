import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { snapshotPage } from "../../../app/agentDiff.fixture";
import { useWorkbenchStore } from "../../../app/store";
import * as acpDesktop from "../../../services/acpDesktop";
import { ChangesSurface } from "../../diff/ChangesSurface";
import { AcpChatPresentation } from "./AcpChatPresentation";
import { resetTurnWorkspaceChangesForTest } from "./turnWorkspaceChanges";

vi.mock("../../../ports/desktopRuntime", () => ({
  isTauriRuntime: () => true,
}));

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  resetTurnWorkspaceChangesForTest();
});

function chatPresentation(
  page: ReturnType<typeof snapshotPage>,
  docked: boolean,
) {
  return (
    <AcpChatPresentation
      page={page}
      hasEarlier={false}
      loadingEarlier={false}
      historyError={null}
      projectRoots={[]}
      retentionIncomplete={false}
      editSummaryContainer={docked ? document.body : undefined}
    />
  );
}

it.each(["truncated"])(
  "marks all change views incomplete when ledger attribution is %s",
  async (kind) => {
    const page = snapshotPage();
    vi.spyOn(acpDesktop, "getTurnWorkspaceChanges").mockResolvedValue({
      rootId: "root-a",
      tracked: kind !== "unavailable",
      truncated: kind === "truncated",
      changes: [],
    });
    const view = render(
      <>
        {chatPresentation(page, true)}
        <ChangesSurface page={page} />
      </>,
    );
    await act(() => vi.dynamicImportSettled());
    expect(
      await screen.findByRole("status", { name: "Workspace tracking" }),
    ).toHaveTextContent("Workspace change tracking is incomplete");
    expect(
      await screen.findByRole("region", { name: "Files edited in this chat" }),
    ).toHaveTextContent("Showing 1 changed file");
    expect(
      await screen.findByText("Showing a bounded partial list of changes."),
    ).toBeVisible();
    expect(screen.queryByText("No changes in this scope.")).toBeNull();
    expect(
      within(
        screen.getByRole("region", { name: "Files edited in this chat" }),
      ).queryByLabelText("Line changes"),
    ).toBeNull();
    expect(
      screen
        .getByRole("region", { name: "Chat changes" })
        .querySelector(".changes-surface__toolbar .diff-header__counts"),
    ).toBeNull();
    view.rerender(
      <>
        {chatPresentation(page, false)}
        <ChangesSurface page={page} />
      </>,
    );
    await act(() => vi.dynamicImportSettled());
    expect(
      await screen.findByRole("region", { name: "Edited files" }),
    ).toHaveTextContent("Workspace change tracking is incomplete");
  },
);

it("keeps a shell deletion after five more terminal turns and across session selection", async () => {
  const page = snapshotPage();
  const original = page.turns[0];
  page.turns = [
    original,
    ...Array.from({ length: 6 }, (_, index) => ({
      ...original,
      id: `turn-${index + 2}` as never,
      ordinal: index + 2,
      items: [],
    })),
  ];
  page.currentSessionId = "new-session" as never;
  page.sessions = [
    ...page.sessions,
    { ...page.sessions[0], id: page.currentSessionId! },
  ];
  vi.spyOn(acpDesktop, "getTurnWorkspaceChanges").mockImplementation(
    async ({ turnId }) => ({
      rootId: "root-a",
      tracked: true,
      truncated: false,
      changes:
        turnId === "turn-2"
          ? [{ relativePath: "src/a.ts", operation: "deleted" }]
          : [],
    }),
  );
  const view = render(chatPresentation(page, true));
  await act(() => vi.dynamicImportSettled());
  const summary = await screen.findByRole("region", {
    name: "Files edited in this chat",
  });
  expect(screen.queryByRole("region", { name: "Edited files" })).toBeNull();
  expect(summary).toHaveTextContent("1 file changed");
  fireEvent.click(within(summary).getByRole("button"));
  expect(summary).toHaveTextContent("deleted");
  expect(summary).not.toHaveTextContent("Saved edit");
  view.rerender(chatPresentation(page, false));
  await act(() => vi.dynamicImportSettled());
  expect(
    await within(screen.getByRole("region", { name: "Turn 2" })).findByRole(
      "region",
      { name: "Edited files" },
    ),
  ).toHaveTextContent("Edited 1 file");
});

it("shows the same shell-only ledger file in the chat summary, Edited files and Last turn", async () => {
  const page = snapshotPage();
  page.turns[0].items = [];
  const load = vi
    .spyOn(acpDesktop, "getTurnWorkspaceChanges")
    .mockResolvedValue({
      rootId: "root-a",
      tracked: true,
      truncated: false,
      changes: [{ relativePath: "shell.ts", operation: "created" }],
    });
  const view = render(
    <>
      {chatPresentation(page, true)}
      <ChangesSurface page={page} />
    </>,
  );
  await act(() => vi.dynamicImportSettled());
  const summary = await screen.findByRole("region", {
    name: "Files edited in this chat",
  });
  expect(summary).toHaveTextContent("1 file changed");
  expect(screen.queryByRole("region", { name: "Edited files" })).toBeNull();
  fireEvent.click(within(summary).getByRole("button"));
  expect(summary).toHaveTextContent("shell.ts");
  const changes = await screen.findByRole("region", {
    name: "Changes shell.ts",
  });
  expect(changes).toHaveTextContent(/did not retain a before\/after pair/);
  // Without a dock host, the same ledger still powers the inline card.
  view.rerender(
    <>
      {chatPresentation(page, false)}
      <ChangesSurface page={page} />
    </>,
  );
  await act(() => vi.dynamicImportSettled());
  const card = await screen.findByRole("region", { name: "Edited files" });
  expect(card).toHaveTextContent("Edited 1 file");
  expect(card).toHaveTextContent("shell.ts");
  expect(
    within(card).getByRole("button", { name: "View changes" }),
  ).toBeDisabled();
  expect(load).toHaveBeenCalledOnce();
  expect(load).toHaveBeenCalledWith(
    expect.objectContaining({
      projectId: page.projectId,
      chatId: page.chatId,
      turnId: page.turns[0].id,
      projectGeneration: String(
        useWorkbenchStore.getState().nativeTransitionEpoch,
      ),
    }),
  );
});
