import { expect, test } from "@playwright/test";
import {
  openAgentScenario,
  openAgentSnapshotDiff,
} from "./resourceVisualHelpers";
import { settleChatProjections } from "./shared/visualBrowserPrimitives";

test.use({ deviceScaleFactor: 1 });

test("Subagents selected mid-turn stays visible after the editing turn completes", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1872, height: 1080 });
  await openAgentScenario(page, "diff");
  await settleChatProjections(page);
  await page.evaluate(async () => {
    const { installPopoverSubagents } =
      await import("/tests/ui/fixtures/popoverSubagentsFixture.ts");
    const { acpProjectionStoreFor } =
      await import("/src/app/acpProjectionRegistry.ts");
    const projection = acpProjectionStoreFor("visual-chat-diff");
    const edits = projection.getState().page!.turns;
    installPopoverSubagents("children");
    const current = projection.getState().page!;
    projection.setState({
      page: {
        ...current,
        turns: [
          {
            ...edits[0],
            state: "running",
            items: [
              ...edits[0].items,
              ...current.turns
                .at(-1)!
                .items.filter((item) => item.kind === "collaboration")
                .map((item) => ({
                  ...item,
                  turnId: edits[0].id,
                  sessionId: edits[0].sessionId,
                })),
            ],
          },
        ],
      },
    });
  });
  await page.getByRole("button", { name: /^Show files and preview/ }).click();
  const pane = page.getByLabel("Agent artifacts", { exact: true });
  const tab = pane.getByRole("tab", { name: "Subagents", exact: true });
  await tab.click();
  await expect(
    pane.getByRole("region", { name: "Subagents", exact: true }),
  ).toBeVisible();
  await page.evaluate(async () => {
    const { acpProjectionStoreFor } =
      await import("/src/app/acpProjectionRegistry.ts");
    const projection = acpProjectionStoreFor("visual-chat-diff");
    const current = projection.getState().page!;
    projection.setState({
      page: {
        ...current,
        turns: current.turns.map((turn) => ({ ...turn, state: "completed" })),
      },
    });
  });
  // Changes arriving proves the completion observer ran, without a timed wait.
  await expect(
    pane.getByRole("tab", { name: "Changes", exact: true }),
  ).toBeVisible();
  await expect(tab).toHaveAttribute("aria-selected", "true");
  await expect(
    pane.getByRole("region", { name: "Subagents", exact: true }),
  ).toBeVisible();
  await expect(
    pane.getByRole("region", { name: "Chat changes", exact: true }),
  ).toHaveCount(0);
});

for (const width of [1872, 800]) {
  test(`the compact Diff row opens its pinned resource in Editor at ${width}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 800 });
    await openAgentScenario(page, "diff");
    await openAgentSnapshotDiff(page);
    const pane = page.getByLabel("Agent artifacts", { exact: true });
    await pane
      .getByRole("tab", {
        name: "Diff src/workbench/AgentChatPanes.tsx",
        exact: true,
      })
      .click();
    await page.evaluate(async () => {
      const { useWorkbenchStore } = await import("/src/app/store.ts");
      await useWorkbenchStore.getState().openFile({
        rootId: "visual-root",
        relativePath: "src/workbench/AgentChatPanes.tsx",
      });
    });
    await expect(
      pane.getByRole("table", { name: "Unified file diff" }),
    ).toBeVisible();
    await pane
      .getByRole("button", { name: "Open in Editor", exact: true })
      .click();
    await expect(
      page.getByRole("radio", { name: "Editor", exact: true }),
    ).toBeChecked();
    await expect(
      page.getByRole("region", {
        name: "Diff for src/workbench/AgentChatPanes.tsx",
        exact: true,
      }),
    ).toBeVisible();
    await expect(
      page.getByRole("table", { name: "Unified file diff" }),
    ).toBeVisible();
  });
}

for (const newerTurn of [false, true]) {
  test(`a delayed turn ledger preserves a selected Diff; newer turn=${newerTurn}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1872, height: 1080 });
    await openAgentScenario(page, "diff");
    await expect(
      page.getByLabel("Chat location", { exact: true }),
    ).toBeVisible();
    await settleChatProjections(page);
    let release: (() => void) | undefined;
    await page.exposeFunction(
      "qaReadTurnChanges",
      () =>
        new Promise((resolve) => {
          release = () =>
            resolve({
              rootId: "visual-root",
              tracked: true,
              truncated: false,
              changes: [{ relativePath: "src/late.ts", operation: "updated" }],
            });
        }),
    );
    await page.evaluate(async () => {
      Object.defineProperty(window, "__TAURI_INTERNALS__", {
        configurable: true,
        value: {
          invoke: async (command: string) => {
            if (command === "get_acp_turn_workspace_changes")
              return (
                window as unknown as {
                  qaReadTurnChanges: () => Promise<unknown>;
                }
              ).qaReadTurnChanges();
            if (command === "get_terminal_dock_visible") return false;
            throw new Error(`Unexpected native fixture command: ${command}`);
          },
        },
      });
      const { acpProjectionStoreFor } =
        await import("/src/app/acpProjectionRegistry.ts");
      const projection = acpProjectionStoreFor("visual-chat-diff");
      const page = projection.getState().page!;
      projection.setState({
        page: { ...page, turns: [{ ...page.turns[0], state: "running" }] },
      });
    });
    await page.evaluate(async () => {
      const { acpProjectionStoreFor } =
        await import("/src/app/acpProjectionRegistry.ts");
      const projection = acpProjectionStoreFor("visual-chat-diff");
      const page = projection.getState().page!;
      projection.setState({
        page: {
          ...page,
          turns: [
            {
              ...page.turns[0],
              state: "completed",
              items: page.turns[0].items.filter(
                (item) => item.kind !== "fileChangeDiff",
              ),
            },
          ],
        },
      });
    });
    await expect.poll(() => Boolean(release)).toBe(true);
    await page.evaluate(async (newer) => {
      const { installVisualAgentLongPathFixture } =
        await import("/tests/ui/fixtures/surfaces/resourceVisualFixtures.ts");
      await installVisualAgentLongPathFixture();
      if (newer) {
        const { acpProjectionStoreFor } =
          await import("/src/app/acpProjectionRegistry.ts");
        const projection = acpProjectionStoreFor("visual-chat-diff");
        const page = projection.getState().page!;
        projection.setState({
          page: {
            ...page,
            turns: [
              ...page.turns,
              {
                ...page.turns[0],
                id: "qa-next-turn" as never,
                ordinal: 2,
                state: "running",
                items: [],
              },
            ],
          },
        });
      }
    }, newerTurn);
    const pane = page.getByLabel("Agent artifacts", { exact: true });
    const diff = pane.getByRole("tab", {
      name: /^Diff src\/workbench\/nested/,
    });
    await diff.click();
    release!();
    await expect(
      pane.getByText("No changes in this scope.", { exact: true }),
    ).toHaveCount(0);
    await expect(diff).toHaveAttribute("aria-selected", "true");
    await expect(
      pane.getByRole("table", { name: "Unified file diff" }),
    ).toBeVisible();
  });
}

test("a completed shell-edit turn opens the native ledger's Changes without a file-change payload", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1872, height: 1080 });
  await openAgentScenario(page, "diff");
  await expect(page.getByLabel("Chat location", { exact: true })).toBeVisible();
  await settleChatProjections(page);
  await page.evaluate(async () => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {
        invoke: async (command: string) => {
          if (command === "get_acp_turn_workspace_changes")
            return {
              rootId: "visual-root",
              tracked: true,
              truncated: false,
              changes: [
                { relativePath: "src/shell-edit.ts", operation: "updated" },
              ],
            };
          if (command === "get_terminal_dock_visible") return false;
          throw new Error(`Unexpected native fixture command: ${command}`);
        },
      },
    });
    const { acpProjectionStoreFor } =
      await import("/src/app/acpProjectionRegistry.ts");
    const projection = acpProjectionStoreFor("visual-chat-diff");
    const page = projection.getState().page!;
    projection.setState({
      page: {
        ...page,
        turns: [
          {
            ...page.turns[0],
            id: "qa-shell-edit-turn",
            state: "running",
            items: page.turns[0].items
              .filter((item) => item.kind !== "fileChangeDiff")
              .map((item) => ({ ...item, turnId: "qa-shell-edit-turn" })),
            activities: page.turns[0].activities.map((activity) => ({
              ...activity,
              turnId: "qa-shell-edit-turn",
            })),
          },
        ],
      },
    });
  });
  await expect(
    page.getByRole("region", { name: "Chat changes", exact: true }),
  ).toHaveCount(0);
  await page.evaluate(async () => {
    const { acpProjectionStoreFor } =
      await import("/src/app/acpProjectionRegistry.ts");
    const projection = acpProjectionStoreFor("visual-chat-diff");
    const page = projection.getState().page!;
    projection.setState({
      page: { ...page, turns: [{ ...page.turns[0], state: "completed" }] },
    });
  });
  const changes = page.getByRole("region", {
    name: "Chat changes",
    exact: true,
  });
  await expect(changes).toBeVisible();
  await expect(
    changes.getByRole("button", { name: /src\/shell-edit.ts/ }),
  ).toBeVisible();
  await page.screenshot({
    path: "local-evidence/qa-after-shell-edit-turn.png",
  });
});

test("an empty working-tree Diff keeps its scope and notice above the content", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1872, height: 1080 });
  await openAgentScenario(page, "diff");
  await expect(page.getByLabel("Chat location", { exact: true })).toBeVisible();
  await settleChatProjections(page);
  await page.evaluate(async () => {
    const { installVisualAgentLongPathFixture } =
      await import("/tests/ui/fixtures/surfaces/resourceVisualFixtures.ts");
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    const { diffCacheKey } = await import("/src/domain/diffIdentity.ts");
    await installVisualAgentLongPathFixture();
    const state = useWorkbenchStore.getState();
    const tab = state.tabs.find((tab) => tab.id === "visual-long-diff-tab")!;
    const key = diffCacheKey(tab.repository, tab.resource!);
    const diff = state.diffs[key];
    if (diff?.phase !== "ready")
      throw new Error("Working Diff fixture unavailable");
    useWorkbenchStore.setState({
      diffs: {
        ...state.diffs,
        [key]: { phase: "ready", value: { ...diff.value, hunks: [] } },
      },
    });
  });
  const pane = page.getByLabel("Agent artifacts", { exact: true });
  await pane.getByRole("tab", { name: /^Diff src\/workbench\/nested/ }).click();
  const scope = pane.getByText("Changes since HEAD (or initial tree)", {
    exact: true,
  });
  const empty = pane.getByText("No textual changes for this file.", {
    exact: true,
  });
  await expect(empty).toBeVisible();
  const top = await pane.locator(".diff-surface").boundingBox();
  const scopeBox = await scope.boundingBox();
  const emptyBox = await empty.boundingBox();
  expect(scopeBox!.y - top!.y).toBeLessThan(48);
  expect(emptyBox!.y - top!.y).toBeLessThan(160);
  await page.screenshot({ path: "local-evidence/qa-after-working-diff.png" });
});

test("wide chat content uses the reading column while the resource pane scales with the window", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1872, height: 1080 });
  await openAgentScenario(page, "diff");
  await openAgentSnapshotDiff(page);
  const column = await page.locator(".chat-dock").boundingBox();
  expect(column!.width).toBeGreaterThanOrEqual(650);
  expect(column!.width).toBeLessThanOrEqual(680);
  const pane = page.getByLabel("Agent artifacts", { exact: true });
  const wide = await pane.boundingBox();
  for (const splitter of await page
    .getByRole("separator", {
      name: /^Resize (project and chat rail|files and preview)$/,
    })
    .all()) {
    expect(
      await splitter.evaluate(
        (node) => getComputedStyle(node, "::after").backgroundColor,
      ),
    ).toBe("rgba(0, 0, 0, 0)");
  }
  await page.screenshot({ path: "local-evidence/qa-after-proportions.png" });
  await page.setViewportSize({ width: 1280, height: 800 });
  await expect
    .poll(async () => (await pane.boundingBox())!.width)
    .toBeLessThan(wide!.width);
  const narrow = await pane.boundingBox();
  expect(wide!.width).toBeGreaterThan(narrow!.width);
  expect(narrow!.width).toBeGreaterThanOrEqual(280);
});

test("approval remains a fully readable warning badge beside a clipped chat title", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await openAgentScenario(page, "waiting");
  await expect(page.getByLabel("Chat location", { exact: true })).toBeVisible();
  await page.evaluate(async () => {
    const { setAgentRailApproval } =
      await import("/src/workbench/agentRailInteractions.ts");
    setAgentRailApproval("visual-chat-waiting", true);
  });
  const badge = page.locator(".rail-pill--approval");
  await expect(badge).toBeVisible();
  const layout = await badge.evaluate((node) => {
    const row = node.closest(".project-chat")!;
    const bounds = node.getBoundingClientRect();
    const rowBounds = row.getBoundingClientRect();
    return {
      fits: bounds.left >= rowBounds.left && bounds.right <= rowBounds.right,
      masked: getComputedStyle(node.parentElement!).maskImage !== "none",
      color: getComputedStyle(node).color,
    };
  });
  expect(layout.fits).toBe(true);
  expect(layout.masked).toBe(false);
  const [red, green, blue] = layout.color.match(/\d+/g)!.map(Number);
  expect(red).toBeGreaterThan(green);
  expect(green).toBeGreaterThan(blue);
  await expect(badge).toHaveText("Awaiting approval");
  await page.screenshot({ path: "local-evidence/qa-after-approval.png" });
  const rail = page.getByRole("separator", {
    name: "Resize project and chat rail",
  });
  await rail.focus();
  await page.keyboard.press("Home");
  await expect(rail).toHaveAttribute("aria-valuenow", "180");
  const row = badge.locator(
    "xpath=ancestor::*[contains(@class, 'project-chat-row')][1]",
  );
  for (const hovered of [false, true]) {
    if (hovered) await row.hover();
    await page.screenshot({
      path: `local-evidence/qa-approval-floor-${hovered}.png`,
    });
    expect(
      await badge.evaluate((node) => {
        const bounds = node.getBoundingClientRect();
        const side = node.closest(".project-sidebar")!.getBoundingClientRect();
        return (
          bounds.left >= side.left &&
          bounds.right <= side.right &&
          node.scrollWidth <= node.clientWidth
        );
      }),
    ).toBe(true);
  }
});

test("Diff content starts below one file row instead of a repeated header stack", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1872, height: 1080 });
  await openAgentScenario(page, "diff");
  await openAgentSnapshotDiff(page);
  const pane = page.getByLabel("Agent artifacts", { exact: true });
  const header = pane.locator(".agent-artifact-pane__header");
  const fileRow = pane.locator(".diff-header");
  await expect(fileRow).toBeVisible();
  const bounds = await fileRow.boundingBox();
  const tabs = await header.boundingBox();
  expect(bounds!.y - tabs!.y - tabs!.height).toBeLessThanOrEqual(1);
  expect(bounds!.height).toBeLessThanOrEqual(48);
  expect(
    (await pane.innerText()).split("src/workbench/AgentChatPanes.tsx"),
  ).toHaveLength(2);
  await expect(pane.getByText("Read-only", { exact: true })).toHaveCount(1);
  await expect(pane.getByText("VIEW ONLY", { exact: true })).toHaveCount(0);
  await expect(
    page.getByRole("button", {
      name: "Open chat Agent navigation polish, pinned",
      exact: true,
    }),
  ).toHaveAttribute("aria-current", "page");
  await page.screenshot({ path: "local-evidence/qa-after-diff.png" });
});

test("at 800px navigation collapses and the resource overlay dismisses through a scrim", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1872, height: 1080 });
  await openAgentScenario(page, "diff");
  await openAgentSnapshotDiff(page);
  await page.setViewportSize({ width: 800, height: 800 });
  await expect(
    page.getByRole("button", { name: "Show projects and chats", exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: /^Show files and preview/ }).click();
  const pane = page.getByRole("dialog", {
    name: "Agent artifacts",
    exact: true,
  });
  await expect(pane).toHaveAttribute("aria-modal", "true");
  const scrim = page.getByRole("button", {
    name: "Dismiss files and preview",
    exact: true,
  });
  await expect(scrim).toBeVisible();
  await expect(page.locator(".title-bar").getByRole("heading")).toHaveCount(0);
  const start = await page.locator(".title-bar__start").boundingBox();
  const search = await page
    .locator(".title-bar__chat-search--pill")
    .boundingBox();
  expect(start!.x + start!.width).toBeLessThanOrEqual(search!.x);
  await page.screenshot({ path: "local-evidence/qa-after-responsive.png" });
  await scrim.click({ position: { x: 80, y: 100 } });
  await expect(pane).toHaveCount(0);
  await page.setViewportSize({ width: 1872, height: 1080 });
  await expect(
    page.getByRole("button", { name: "Hide projects and chats", exact: true }),
  ).toBeVisible();
});

test("a completed editing turn opens its Changes without a resource click", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1872, height: 1080 });
  await openAgentScenario(page, "diff");
  await expect(page.getByLabel("Chat location", { exact: true })).toBeVisible();
  await page.evaluate(async () => {
    const { acpProjectionStoreFor } =
      await import("/src/app/acpProjectionRegistry.ts");
    const projection = acpProjectionStoreFor("visual-chat-diff");
    const page = projection.getState().page!;
    projection.setState({
      page: {
        ...page,
        turns: [
          {
            ...page.turns[0],
            id: "qa-payload-edit-turn",
            state: "running",
            items: page.turns[0].items.map((item) => ({
              ...item,
              turnId: "qa-payload-edit-turn",
            })),
            activities: page.turns[0].activities.map((activity) => ({
              ...activity,
              turnId: "qa-payload-edit-turn",
            })),
          },
        ],
      },
    });
  });
  await expect(
    page.getByRole("region", { name: "Chat changes", exact: true }),
  ).toHaveCount(0);
  await page.evaluate(async () => {
    const { acpProjectionStoreFor } =
      await import("/src/app/acpProjectionRegistry.ts");
    const projection = acpProjectionStoreFor("visual-chat-diff");
    const page = projection.getState().page!;
    projection.setState({
      page: { ...page, turns: [{ ...page.turns[0], state: "completed" }] },
    });
  });
  const changes = page.getByRole("region", {
    name: "Chat changes",
    exact: true,
  });
  await expect(changes).toBeVisible();
  await expect(
    changes.getByRole("button", { name: /src\/workbench\/AgentChatPanes.tsx/ }),
  ).toBeVisible();
  await expect(
    changes.locator(".changes-surface__toolbar .diff-header__counts"),
  ).toHaveText("+1−1");
  await page.screenshot({ path: "local-evidence/qa-after-turn.png" });
});
