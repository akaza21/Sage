import { expect, test, type Page } from "@playwright/test";

async function savedChats(page: Page) {
  return page.evaluate(async () => {
    const { getWorkbenchPorts } =
      await import("/src/ports/workbenchRuntime.ts");
    const snapshot = await getWorkbenchPorts().project.getBootstrapSnapshot();
    return {
      tabs: snapshot.tabs,
      chats: snapshot.tabs.filter((tab) => tab.kind === "chat"),
      archived: snapshot.archivedChats,
      active: snapshot.activeTabId,
      project: snapshot.project,
      messages: snapshot.messages,
    };
  });
}

for (const collapsed of [false, true]) {
  test(`Home opens a focused draft beside inline resources; collapsed=${collapsed}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 720, height: 1000 });
    await page.goto("/");
    const composer = page.getByRole("combobox", { name: "Message agent" });
    await expect(composer).toBeVisible();
    const before = await savedChats(page);
    if (collapsed)
      await page
        .getByRole("button", { name: "Hide projects and chats" })
        .click();
    await composer.fill("Inline Home draft");
    await page.getByRole("button", { name: /Show files and preview/ }).click();
    // At 720px the resources dock beside Chat instead of covering it.
    const resources = page.getByLabel("Agent artifacts", { exact: true });
    await expect(resources).toBeVisible();
    await expect(
      page.getByRole("dialog", { name: "Agent artifacts" }),
    ).toHaveCount(0);
    await expect(page.locator(".center-region")).not.toHaveAttribute("inert");
    await page.getByRole("button", { name: "Home", exact: true }).click();
    const draft = page.getByRole("region", { name: "New chat draft" });
    await expect(
      draft.getByRole("heading", { name: "What should we work on?" }),
    ).toBeVisible();
    await expect(
      draft.getByRole("combobox", { name: "Message agent" }),
    ).toBeFocused();
    await expect(
      draft.getByRole("combobox", { name: "Message agent" }),
    ).toHaveValue("");
    await expect(page.getByRole("region", { name: /^Chat pane / })).toHaveCount(
      0,
    );
    await expect(
      page.locator('.project-chat[aria-current="page"]'),
    ).toHaveCount(0);
    await expect(resources).toBeVisible();
    expect(await savedChats(page)).toEqual(before);
  });

  test(`Home preserves the saved conversation; collapsed=${collapsed}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1800, height: 1000 });
    await page.goto("/");
    const composer = page.getByRole("combobox", { name: "Message agent" });
    await expect(composer).toBeVisible();
    const before = await savedChats(page);
    expect(before.chats).toHaveLength(1);
    expect(before.messages.length).toBeGreaterThan(0);
    await expect(
      page.getByText(before.messages[0].text, { exact: true }),
    ).toBeVisible();
    const transcript = page.locator(".message-list__content");
    const savedTranscript = await transcript.innerText();
    expect(savedTranscript.trim()).not.toBe("");
    if (collapsed)
      await page
        .getByRole("button", { name: "Hide projects and chats" })
        .click();
    await composer.fill("Unsent Home navigation check");
    for (let click = 0; click < 5; click += 1)
      await page.getByRole("button", { name: "Home", exact: true }).click();
    expect(await savedChats(page)).toEqual(before);
    const draft = page.getByRole("region", { name: "New chat draft" });
    const draftComposer = draft.getByRole("combobox", {
      name: "Message agent",
    });
    await expect(
      draft.getByRole("heading", { name: "What should we work on?" }),
    ).toBeVisible();
    await expect(draftComposer).toBeFocused();
    await expect(draftComposer).toHaveValue("");
    await expect(page.getByRole("region", { name: /^Chat pane / })).toHaveCount(
      0,
    );
    await expect(
      page.locator('.project-chat[aria-current="page"]'),
    ).toHaveCount(0);
    await draftComposer.fill("Unsaved Home text");
    await page.getByRole("button", { name: "Home", exact: true }).click();
    await expect(draftComposer).toBeFocused();
    await expect(draftComposer).toHaveValue("Unsaved Home text");
    await expect(page.getByText(/New chat [2-9]/)).toHaveCount(0);
    await expect(page.getByRole("radio", { name: "Agent" })).toBeChecked();
    await page
      .getByRole("button", {
        name: collapsed ? "Open settings from rail" : "Open settings",
        exact: true,
      })
      .click();
    await page.getByRole("button", { name: "Close settings" }).click();
    await expect(draftComposer).toHaveValue("Unsaved Home text");
    expect(await savedChats(page)).toEqual(before);
    await page
      .getByRole("button", {
        name: "New chat",
        exact: true,
      })
      .click();
    await expect(draftComposer).toBeFocused();
    await expect(draftComposer).toHaveValue("Unsaved Home text");
    await expect(page.getByRole("region", { name: /^Chat pane / })).toHaveCount(
      0,
    );
    expect(await savedChats(page)).toEqual(before);
    if (collapsed)
      await page
        .getByRole("button", { name: "Show projects and chats" })
        .click();
    const previousChat = page.locator(
      `.project-chat[data-chat-id="${before.active}"]`,
    );
    await expect(previousChat).toBeVisible();
    await expect(
      page.locator('.project-chat[aria-current="page"]'),
    ).toHaveCount(0);
    await previousChat.click();
    await expect(draft).toHaveCount(0);
    const restoredPane = page.getByRole("region", { name: /^Chat pane / });
    await expect(restoredPane).toHaveCount(1);
    await expect(restoredPane).toHaveAttribute("data-chat-id", before.active!);
    await expect(previousChat).toHaveAttribute("aria-current", "page");
    await expect(composer).toHaveValue("Unsent Home navigation check");
    await expect(transcript).toHaveText(savedTranscript, {
      useInnerText: true,
    });
    expect(await savedChats(page)).toEqual(before);
  });
}

for (const owner of ["single", "parent", "side"] as const) {
  test(`Home focuses the draft composer when the ${owner} Chat composer is disabled`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 2400, height: 1000 });
    await page.goto("/");
    await expect(
      page.getByRole("combobox", { name: "Message agent" }),
    ).toBeVisible();
    await page.evaluate(async (owner) => {
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
        harnesses: [
          ...state.harnesses,
          {
            ...state.harnesses[0]!,
            id: "fixture-unavailable",
            installState: "unavailable",
          },
        ],
        chatUiById: {
          ...state.chatUiById,
          [parentId]: {
            ...state.chatUiById[parentId],
            composerText: "Parent draft",
          },
          [focusedId]: {
            ...state.chatUiById[focusedId],
            composerText: "Focused draft",
            harnessId: "fixture-unavailable",
          },
        },
      }));
    }, owner);
    const before = await savedChats(page);
    const panes = page.getByRole("region", { name: /^Chat pane / });
    await expect(panes).toHaveCount(owner === "single" ? 1 : 2);
    const focused = page.locator(".agent-chat-pane--focused");
    await expect(
      focused.getByRole("combobox", { name: "Message agent" }),
    ).toBeDisabled();
    if (owner !== "single")
      await expect(
        page
          .locator(".agent-chat-pane:not(.agent-chat-pane--focused)")
          .getByRole("combobox", { name: "Message agent" }),
      ).toBeEnabled();
    const home = page.getByRole("button", { name: "Home", exact: true });
    if (owner === "single") {
      await home.focus();
      await page.keyboard.press("Enter");
    } else await home.click();
    await expect(panes).toHaveCount(0);
    const draft = page.getByRole("region", { name: "New chat draft" });
    await expect(
      draft.getByRole("heading", { name: "What should we work on?" }),
    ).toBeVisible();
    const draftComposer = draft.getByRole("combobox", {
      name: "Message agent",
    });
    await expect(draftComposer).toBeEnabled();
    await expect(draftComposer).toBeFocused();
    await expect(draftComposer).toHaveValue("");
    await expect(
      page.locator('.project-chat[aria-current="page"]'),
    ).toHaveCount(0);
    expect(await savedChats(page)).toEqual(before);
  });
}
