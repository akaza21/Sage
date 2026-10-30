import { expect, test, type Page } from "@playwright/test";

async function savedChats(page: Page) {
  return page.evaluate(async () => {
    const { getWorkbenchPorts } =
      await import("/src/ports/workbenchRuntime.ts");
    const snapshot = await getWorkbenchPorts().project.getBootstrapSnapshot();
    return {
      chats: snapshot.tabs.filter((tab) => tab.kind === "chat"),
      archived: snapshot.archivedChats,
      active: snapshot.activeTabId,
      project: snapshot.project,
      messages: snapshot.messages,
    };
  });
}

for (const collapsed of [false, true]) {
  test(`Home dismisses nonmodal resources before focusing Chat; collapsed=${collapsed}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 720, height: 1000 });
    await page.goto("/");
    const composer = page.getByRole("textbox", { name: "Message agent" });
    await expect(composer).toBeVisible();
    const before = await savedChats(page);
    if (collapsed)
      await page
        .getByRole("button", { name: "Hide projects and chats" })
        .click();
    await composer.fill("Overlay Home draft");
    await page.getByRole("button", { name: /Show files and preview/ }).click();
    const resources = page.getByRole("dialog", { name: "Agent artifacts" });
    await expect(resources).toBeVisible();
    await expect(resources).not.toHaveAttribute("aria-modal");
    await expect(page.locator(".center-region")).toHaveAttribute("inert");
    await page.getByRole("button", { name: "Home", exact: true }).click();
    await expect(resources).toHaveCount(0);
    await expect(page.locator(".center-region")).not.toHaveAttribute("inert");
    await expect(composer).toBeFocused();
    await expect(composer).toHaveValue("Overlay Home draft");
    expect(await savedChats(page)).toEqual(before);
  });

  test(`Home preserves the saved conversation; collapsed=${collapsed}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1800, height: 1000 });
    await page.goto("/");
    const composer = page.getByRole("textbox", { name: "Message agent" });
    await expect(composer).toBeVisible();
    const before = await savedChats(page);
    expect(before.chats).toHaveLength(1);
    expect(before.messages.length).toBeGreaterThan(0);
    if (collapsed)
      await page
        .getByRole("button", { name: "Hide projects and chats" })
        .click();
    await composer.fill("Unsent Home navigation check");
    for (let click = 0; click < 5; click += 1)
      await page.getByRole("button", { name: "Home", exact: true }).click();
    expect(await savedChats(page)).toEqual(before);
    await expect(composer).toBeFocused();
    await expect(composer).toHaveValue("Unsent Home navigation check");
    await expect(page.getByText(/New chat [2-9]/)).toHaveCount(0);
    await expect(page.getByRole("radio", { name: "Agent" })).toBeChecked();
    await page
      .getByRole("button", {
        name: collapsed ? "Open settings from rail" : "Open settings",
        exact: true,
      })
      .click();
    await page.getByRole("button", { name: "Back to chats" }).click();
    await expect(composer).toHaveValue("Unsent Home navigation check");
    expect(await savedChats(page)).toEqual(before);
    await page
      .getByRole("button", {
        name: collapsed ? "New chat" : "New task",
        exact: true,
      })
      .click();
    await expect
      .poll(async () => (await savedChats(page)).chats.length)
      .toBe(2);
    const created = await savedChats(page);
    expect(created.active).not.toBe(before.active);
    expect(created.messages).toEqual([]);
    expect(created.project).toEqual(before.project);
    expect(created.chats.find((chat) => chat.id === before.active)).toEqual(
      before.chats[0],
    );
  });
}

for (const owner of ["single", "parent", "side"] as const) {
  test(`Home focuses the ${owner} Chat region when its composer is disabled`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 2400, height: 1000 });
    await page.goto("/");
    await expect(
      page.getByRole("textbox", { name: "Message agent" }),
    ).toBeVisible();
    const focusedId = await page.evaluate(async (owner) => {
      const { useWorkbenchStore } = await import("/src/app/store.ts");
      const { useSideChatState } =
        await import("/src/workbench/sideChatState.ts");
      const parentId = useWorkbenchStore.getState().activeTabId!;
      if (owner !== "single") {
        await useWorkbenchStore.getState().createChat();
        const sideId = useWorkbenchStore.getState().activeTabId!;
        useSideChatState.setState({
          sides: [
            {
              id: sideId,
              parentChatId: parentId,
              projectId: useWorkbenchStore.getState().project!.id,
            },
          ],
          pairedId: sideId,
        });
        await useWorkbenchStore
          .getState()
          .activateTab(owner === "parent" ? parentId : sideId);
        useWorkbenchStore.setState((state) => ({
          chatUiById: {
            ...state.chatUiById,
            [sideId]: {
              ...state.chatUiById[sideId],
              composerText: "Side draft",
            },
          },
        }));
      }
      const focusedId = useWorkbenchStore.getState().activeTabId!;
      useWorkbenchStore.setState((state) => ({
        chatUiById: {
          ...state.chatUiById,
          [parentId]: {
            ...state.chatUiById[parentId],
            composerText: "Parent draft",
          },
          [focusedId]: {
            ...state.chatUiById[focusedId],
            composerText: "Focused draft",
            sending: true,
          },
        },
      }));
      return focusedId;
    }, owner);
    const before = await savedChats(page);
    const panes = page.getByRole("region", { name: /^Chat pane / });
    await expect(panes).toHaveCount(owner === "single" ? 1 : 2);
    const focused = page.locator(".agent-chat-pane--focused");
    await expect(
      focused.getByRole("textbox", { name: "Message agent" }),
    ).toBeDisabled();
    if (owner !== "single")
      await expect(
        page
          .locator(".agent-chat-pane:not(.agent-chat-pane--focused)")
          .getByRole("textbox"),
      ).toBeEnabled();
    const home = page.getByRole("button", { name: "Home", exact: true });
    if (owner === "single") {
      await home.focus();
      await page.keyboard.press("Enter");
    } else await home.click();
    await expect(focused).toBeFocused();
    if (owner === "single")
      await expect(focused).toHaveCSS("outline-style", "solid");
    await expect(focused).toHaveAttribute("data-chat-id", focusedId);
    await expect(focused).toHaveAccessibleName(/^Chat pane /);
    await expect(focused.getByRole("textbox")).toHaveValue("Focused draft");
    expect(await savedChats(page)).toEqual(before);
  });
}
