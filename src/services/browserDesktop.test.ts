import { beforeEach, describe, expect, it } from "vitest";

import { MAX_CHAT_TITLE_BYTES } from "../domain/chatTitle";
import type { ChatMessage } from "../domain/workbench";
import {
  appendBrowserSimulatorTurn,
  browserDesktop,
  resetBrowserDesktopForTests,
} from "./browserDesktop";

function messages(count: number, text = "x"): ChatMessage[] {
  return Array.from({ length: count }, (_, index) => ({
    id: `message-${index}`,
    sessionId: "session",
    sequence: index + 1,
    role: "user",
    text,
    createdAt: 1,
  }));
}

const request = { sessionId: "session", modelId: "sim-fast", text: "hello" };

describe("browser simulator history bounds", () => {
  beforeEach(() => resetBrowserDesktopForTests());

  it("returns the authoritative simulator session for a Chat-addressed prompt", async () => {
    const snapshot = await browserDesktop.getBootstrapSnapshot();
    expect(snapshot.sessionId).toBeNull();
    const result = await browserDesktop.sendPrompt({
      chatId: snapshot.activeTabId!,
      harnessId: "sage-simulator",
      modelId: "sim-fast",
      text: "continue",
    });

    expect(result.sessionId).toEqual(expect.any(String));
    expect(result.messages.at(-1)).toMatchObject({
      sessionId: result.sessionId,
      role: "assistant",
    });
    expect((await browserDesktop.getBootstrapSnapshot()).sessionId).toBe(
      result.sessionId,
    );
  });

  it("creates and isolates two browser Chats through authoritative projections", async () => {
    const initial = await browserDesktop.getBootstrapSnapshot();
    const created = await browserDesktop.createChat(
      {
        projectId: initial.project!.id,
        tabs: initial.tabs,
        activeTabId: initial.activeTabId,
        selectedRootId: initial.selectedRootId,
        selectedResource: initial.selectedResource,
      },
      initial.transitionEpoch,
    );
    const secondChatId = created.snapshot!.activeTabId!;
    const second = await browserDesktop.sendPrompt({
      chatId: secondChatId,
      harnessId: "sage-simulator",
      modelId: "sim-fast",
      text: "second-only",
    });
    const first = await browserDesktop.sendPrompt({
      chatId: initial.activeTabId!,
      harnessId: "sage-simulator",
      modelId: "sim-fast",
      text: "first-only",
    });

    expect(second.messages.map((message) => message.text)).toContain(
      "second-only",
    );
    expect(second.messages.map((message) => message.text)).not.toContain(
      "first-only",
    );
    expect(first.messages.map((message) => message.text)).toContain(
      "first-only",
    );
    expect(first.messages.map((message) => message.text)).not.toContain(
      "second-only",
    );

    const firstProjection = await browserDesktop.activateChat(
      {
        projectId: initial.project!.id,
        tabs: created.snapshot!.tabs,
        activeTabId: initial.activeTabId,
        selectedRootId: initial.selectedRootId,
        selectedResource: initial.selectedResource,
      },
      created.transitionEpoch,
    );
    expect(firstProjection.snapshot).toMatchObject({
      activeTabId: initial.activeTabId,
      sessionId: first.sessionId,
      messages: first.messages,
    });
    const secondProjection = await browserDesktop.activateChat(
      {
        projectId: initial.project!.id,
        tabs: created.snapshot!.tabs,
        activeTabId: secondChatId,
        selectedRootId: initial.selectedRootId,
        selectedResource: initial.selectedResource,
      },
      firstProjection.transitionEpoch,
    );
    expect(secondProjection.snapshot).toMatchObject({
      activeTabId: secondChatId,
      sessionId: second.sessionId,
      messages: second.messages,
    });
  });

  it("renames active and inactive Chats without changing identity or history", async () => {
    const initial = await browserDesktop.getBootstrapSnapshot();
    const created = await browserDesktop.createChat(
      {
        projectId: initial.project!.id,
        tabs: initial.tabs,
        activeTabId: initial.activeTabId,
        selectedRootId: initial.selectedRootId,
        selectedResource: initial.selectedResource,
      },
      initial.transitionEpoch,
    );
    const secondChatId = created.snapshot!.activeTabId!;
    const turn = await browserDesktop.sendPrompt({
      chatId: secondChatId,
      harnessId: "sage-simulator",
      modelId: "sim-fast",
      text: "history stays",
    });
    const identity = created.snapshot!.tabs.map((tab) => ({
      id: tab.id,
      position: tab.position,
    }));

    const inactive = await browserDesktop.renameChat(
      {
        projectId: initial.project!.id,
        chatId: initial.activeTabId!,
        title: "  Research  ",
      },
      created.transitionEpoch,
    );
    expect(inactive.snapshot).toMatchObject({
      activeTabId: secondChatId,
      sessionId: turn.sessionId,
      messages: turn.messages,
    });
    expect(
      inactive.snapshot!.tabs.find((tab) => tab.id === initial.activeTabId)
        ?.title,
    ).toBe("Chat: Research");

    const active = await browserDesktop.renameChat(
      {
        projectId: initial.project!.id,
        chatId: secondChatId,
        title: "Delivery",
      },
      inactive.transitionEpoch,
    );
    expect(active.snapshot).toMatchObject({
      activeTabId: secondChatId,
      sessionId: turn.sessionId,
      messages: turn.messages,
    });
    expect(
      active.snapshot!.tabs.map((tab) => ({
        id: tab.id,
        position: tab.position,
      })),
    ).toEqual(identity);
    expect(
      active.snapshot!.tabs.find((tab) => tab.id === secondChatId)?.title,
    ).toBe("Chat: Delivery");
  });

  it("rejects stale foreign invalid and over-limit rename before mutation", async () => {
    const initial = await browserDesktop.getBootstrapSnapshot();
    const exact = "é".repeat(MAX_CHAT_TITLE_BYTES / 2);
    const renamed = await browserDesktop.renameChat(
      {
        projectId: initial.project!.id,
        chatId: initial.activeTabId!,
        title: exact,
      },
      initial.transitionEpoch,
    );
    expect(renamed.snapshot!.tabs[0].title).toBe(`Chat: ${exact}`);

    for (const [request, epoch] of [
      [
        {
          projectId: initial.project!.id,
          chatId: initial.activeTabId!,
          title: `${exact}x`,
        },
        renamed.transitionEpoch,
      ],
      [
        {
          projectId: initial.project!.id,
          chatId: initial.activeTabId!,
          title: " \t ",
        },
        renamed.transitionEpoch,
      ],
      [
        {
          projectId: initial.project!.id,
          chatId: initial.activeTabId!,
          title: "bad\nname",
        },
        renamed.transitionEpoch,
      ],
      [
        {
          projectId: "foreign-project",
          chatId: initial.activeTabId!,
          title: "Foreign",
        },
        renamed.transitionEpoch,
      ],
      [
        {
          projectId: initial.project!.id,
          chatId: "foreign-chat",
          title: "Foreign",
        },
        renamed.transitionEpoch,
      ],
      [
        {
          projectId: initial.project!.id,
          chatId: initial.activeTabId!,
          title: "Stale",
        },
        initial.transitionEpoch,
      ],
    ] as const) {
      await expect(browserDesktop.renameChat(request, epoch)).rejects.toThrow();
      expect(await browserDesktop.getBootstrapSnapshot()).toEqual(
        renamed.snapshot,
      );
    }
  });

  it("fails closed at 64 durable browser Chats without evicting detached history", async () => {
    const initial = await browserDesktop.getBootstrapSnapshot();
    const initialTab = initial.tabs[0];
    let transitionEpoch = initial.transitionEpoch;
    let lastProjection = initial;
    for (let index = 1; index < 64; index += 1) {
      const created = await browserDesktop.createChat(
        {
          projectId: initial.project!.id,
          tabs: [initialTab],
          activeTabId: initialTab.id,
          selectedRootId: initial.selectedRootId,
          selectedResource: null,
        },
        transitionEpoch,
      );
      transitionEpoch = created.transitionEpoch;
      lastProjection = created.snapshot!;
    }

    await expect(
      browserDesktop.createChat(
        {
          projectId: initial.project!.id,
          tabs: [initialTab],
          activeTabId: initialTab.id,
          selectedRootId: initial.selectedRootId,
          selectedResource: null,
        },
        transitionEpoch,
      ),
    ).rejects.toThrow("64-Chat limit; existing Chats were preserved");
    expect(await browserDesktop.getBootstrapSnapshot()).toEqual(lastProjection);

    const restored = await browserDesktop.activateChat(
      {
        projectId: initial.project!.id,
        tabs: [initialTab],
        activeTabId: initialTab.id,
        selectedRootId: initial.selectedRootId,
        selectedResource: null,
      },
      transitionEpoch,
    );
    expect(restored.snapshot!.messages).toEqual(initial.messages);
  });

  it("exposes the four curated harnesses in product admission order", async () => {
    const snapshot = await browserDesktop.getBootstrapSnapshot();
    expect(snapshot.harnesses.slice(0, 4).map((harness) => harness.id)).toEqual(
      ["codex", "pi", "claude-code", "oh-my-pi"],
    );
  });

  it("mints a new simulator attempt when the selected model changes", async () => {
    expect(await browserDesktop.listHarnessModels("sage-simulator")).toEqual(
      expect.arrayContaining([expect.objectContaining({ id: "sim-deep" })]),
    );
    const first = await browserDesktop.sendPrompt({
      chatId: "tab-chat",
      harnessId: "sage-simulator",
      modelId: "sim-fast",
      text: "fast",
    });
    const second = await browserDesktop.sendPrompt({
      chatId: "tab-chat",
      harnessId: "sage-simulator",
      modelId: "sim-deep",
      text: "deep",
    });

    expect(second.sessionId).not.toBe(first.sessionId);
    expect(second.messages).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ sessionId: first.sessionId, text: "fast" }),
        expect.objectContaining({ sessionId: second.sessionId, text: "deep" }),
      ]),
    );
    expect((await browserDesktop.getBootstrapSnapshot()).selectedModelId).toBe(
      "sim-deep",
    );
  });

  it("rejects unadvertised browser harness and model identity before mutation", async () => {
    const before = await browserDesktop.getBootstrapSnapshot();
    await expect(
      browserDesktop.sendPrompt({
        chatId: "tab-chat",
        harnessId: "sage-simulator",
        modelId: "foreign-model",
        text: "must fail",
      }),
    ).rejects.toThrow("not available");
    expect(await browserDesktop.getBootstrapSnapshot()).toEqual(before);
  });

  it("exposes explicit transport and only admitted ACP binding identity", async () => {
    const snapshot = await browserDesktop.getBootstrapSnapshot();
    const [codex, pi, claude, omp, simulator] = snapshot.harnesses;
    expect(codex).toMatchObject({
      transport: "acp",
      bindingId: "codex-app-server",
    });
    expect([pi, claude, omp]).toEqual([
      expect.objectContaining({ transport: "acp", bindingId: null }),
      expect.objectContaining({ transport: "acp", bindingId: null }),
      expect.objectContaining({ transport: "acp", bindingId: null }),
    ]);
    expect(simulator).toMatchObject({
      transport: "legacySimulator",
      bindingId: null,
    });
  });

  it("accepts an exact 500-message projection then rejects a whole turn", () => {
    const exact = appendBrowserSimulatorTurn(
      messages(498),
      request,
      ["user", "assistant"],
      2,
    );
    expect(exact).toHaveLength(500);
    expect(() =>
      appendBrowserSimulatorTurn(exact, request, [
        "next-user",
        "next-assistant",
      ]),
    ).toThrow("existing messages were preserved");
    expect(exact).toHaveLength(500);
  });

  it("counts UTF-8 bytes and rejects before mutating the prior projection", () => {
    const existing = messages(1, "é".repeat(2 * 1024 * 1024));
    expect(() =>
      appendBrowserSimulatorTurn(existing, { ...request, text: "overflow" }, [
        "user",
        "assistant",
      ]),
    ).toThrow("capacity reached");
    expect(existing).toHaveLength(1);
  });
});
