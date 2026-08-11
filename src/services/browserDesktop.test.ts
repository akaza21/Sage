import { beforeEach, describe, expect, it, vi } from "vitest";

import { MAX_CHAT_TITLE_BYTES } from "../domain/chatTitle";
import type {
  ChatMessage,
  ProjectLayoutRequest,
  WorkbenchTab,
} from "../domain/workbench";
import { assertBrowserChatRenameIdentity } from "./browserChat";
import {
  appendBrowserSimulatorTurn,
  BrowserGitMutationUnavailableError,
  BrowserFileRenameUnavailableError,
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

describe("browser repository mutations", () => {
  it("reports file rename as a typed native-only operation", async () => {
    await expect(
      browserDesktop.renameProjectFile({
        projectId: "project-a",
        projectGeneration: 1,
        expectedTransitionEpoch: 0,
        source: { rootId: "root-a", relativePath: "src/old.ts" },
        basename: "new.ts",
        currentLayout: {
          projectId: "project-a",
          tabs: [],
          activeTabId: null,
          selectedRootId: "root-a",
          selectedResource: null,
        },
      }),
    ).rejects.toBeInstanceOf(BrowserFileRenameUnavailableError);
  });

  it("reports a typed native-only failure without simulating Git", async () => {
    await expect(
      browserDesktop.mutateGitFile(
        {
          projectId: "project-a",
          resource: { rootId: "root-a", relativePath: "file.txt" },
          action: "stage",
          discardConfirmed: false,
        },
        0,
      ),
    ).rejects.toBeInstanceOf(BrowserGitMutationUnavailableError);
  });
});

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

  it("renames Chats while atomically preserving the current layout and history", async () => {
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
    const selectedResource = {
      rootId: initial.selectedRootId!,
      relativePath: "src/current.ts",
    };
    const fileTab: WorkbenchTab = {
      id: "file:current",
      kind: "file",
      title: "current.ts",
      position: 2,
      pinned: false,
      dirty: true,
      status: "idle",
      resource: selectedResource,
    };
    const currentLayout: ProjectLayoutRequest = {
      projectId: initial.project!.id,
      tabs: [...created.snapshot!.tabs, fileTab],
      activeTabId: fileTab.id,
      selectedRootId: selectedResource.rootId,
      selectedResource,
    };
    const identity = currentLayout.tabs.map((tab) => ({
      id: tab.id,
      position: tab.position,
    }));

    const inactive = await browserDesktop.renameChat(
      {
        projectId: initial.project!.id,
        chatId: initial.activeTabId!,
        title: "  Research  ",
        currentLayout,
      },
      created.transitionEpoch,
    );
    expect(inactive.snapshot).toMatchObject({
      activeTabId: fileTab.id,
      selectedRootId: selectedResource.rootId,
      selectedResource,
      sessionId: null,
      messages: [],
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
        currentLayout: {
          projectId: initial.project!.id,
          tabs: inactive.snapshot!.tabs,
          activeTabId: inactive.snapshot!.activeTabId,
          selectedRootId: inactive.snapshot!.selectedRootId,
          selectedResource: inactive.snapshot!.selectedResource,
        },
      },
      inactive.transitionEpoch,
    );
    expect(active.snapshot).toMatchObject({
      activeTabId: fileTab.id,
      selectedResource,
      sessionId: null,
      messages: [],
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

    const reactivated = await browserDesktop.activateChat(
      {
        projectId: initial.project!.id,
        tabs: active.snapshot!.tabs,
        activeTabId: secondChatId,
        selectedRootId: active.snapshot!.selectedRootId,
        selectedResource: active.snapshot!.selectedResource,
      },
      active.transitionEpoch,
    );
    expect(reactivated.snapshot).toMatchObject({
      activeTabId: secondChatId,
      sessionId: turn.sessionId,
      messages: turn.messages,
    });
  });

  it("archives and restores a durable Chat without losing its history", async () => {
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
    const request = {
      projectId: initial.project!.id,
      chatId: initial.activeTabId!,
      currentLayout: {
        projectId: initial.project!.id,
        tabs: created.snapshot!.tabs,
        activeTabId: created.snapshot!.activeTabId,
        selectedRootId: created.snapshot!.selectedRootId,
        selectedResource: created.snapshot!.selectedResource,
      },
    };

    const archived = await browserDesktop.archiveChat(
      request,
      created.transitionEpoch,
    );
    expect(archived.snapshot).toMatchObject({
      activeTabId: secondChatId,
      archivedChats: [
        { id: initial.activeTabId, title: "Chat: Walking skeleton" },
      ],
    });
    expect(archived.snapshot!.tabs.map((tab) => tab.id)).toEqual([
      secondChatId,
    ]);
    await expect(
      browserDesktop.restoreChat(request, created.transitionEpoch),
    ).rejects.toThrow("project changed before the requested Chat restore");

    const restored = await browserDesktop.restoreChat(
      {
        ...request,
        currentLayout: {
          ...request.currentLayout,
          tabs: archived.snapshot!.tabs,
          activeTabId: archived.snapshot!.activeTabId,
        },
      },
      archived.transitionEpoch,
    );
    expect(restored.snapshot).toMatchObject({
      activeTabId: initial.activeTabId,
      archivedChats: [],
      messages: initial.messages,
    });
  });

  it("matches native archive selection and restore rejection boundaries", async () => {
    const initial = await browserDesktop.getBootstrapSnapshot();
    const firstChatId = initial.activeTabId!;
    const second = await browserDesktop.createChat(
      {
        projectId: initial.project!.id,
        tabs: initial.tabs,
        activeTabId: initial.activeTabId,
        selectedRootId: initial.selectedRootId,
        selectedResource: initial.selectedResource,
      },
      initial.transitionEpoch,
    );
    const secondChatId = second.snapshot!.activeTabId!;
    const third = await browserDesktop.createChat(
      {
        projectId: initial.project!.id,
        tabs: second.snapshot!.tabs,
        activeTabId: second.snapshot!.activeTabId,
        selectedRootId: second.snapshot!.selectedRootId,
        selectedResource: second.snapshot!.selectedResource,
      },
      second.transitionEpoch,
    );
    const thirdChatId = third.snapshot!.activeTabId!;
    const archivedMiddle = await browserDesktop.archiveChat(
      {
        projectId: initial.project!.id,
        chatId: secondChatId,
        currentLayout: {
          projectId: initial.project!.id,
          tabs: third.snapshot!.tabs,
          activeTabId: secondChatId,
          selectedRootId: third.snapshot!.selectedRootId,
          selectedResource: third.snapshot!.selectedResource,
        },
      },
      third.transitionEpoch,
    );
    expect(archivedMiddle.snapshot!.activeTabId).toBe(thirdChatId);

    const archivedLast = await browserDesktop.archiveChat(
      {
        projectId: initial.project!.id,
        chatId: thirdChatId,
        currentLayout: {
          projectId: initial.project!.id,
          tabs: archivedMiddle.snapshot!.tabs,
          activeTabId: archivedMiddle.snapshot!.activeTabId,
          selectedRootId: archivedMiddle.snapshot!.selectedRootId,
          selectedResource: archivedMiddle.snapshot!.selectedResource,
        },
      },
      archivedMiddle.transitionEpoch,
    );
    expect(archivedLast.snapshot!.activeTabId).toBe(firstChatId);
    const oneChatLayout = {
      projectId: initial.project!.id,
      tabs: archivedLast.snapshot!.tabs,
      activeTabId: archivedLast.snapshot!.activeTabId,
      selectedRootId: archivedLast.snapshot!.selectedRootId,
      selectedResource: archivedLast.snapshot!.selectedResource,
    };
    const beforeRejectedIdentities =
      await browserDesktop.getBootstrapSnapshot();
    await expect(
      browserDesktop.archiveChat(
        {
          projectId: initial.project!.id,
          chatId: firstChatId,
          currentLayout: oneChatLayout,
        },
        archivedLast.transitionEpoch,
      ),
    ).rejects.toThrow("last attached Chat");
    await expect(
      browserDesktop.restoreChat(
        {
          projectId: initial.project!.id,
          chatId: firstChatId,
          currentLayout: oneChatLayout,
        },
        archivedLast.transitionEpoch,
      ),
    ).rejects.toThrow("already attached");
    await expect(
      browserDesktop.restoreChat(
        {
          projectId: initial.project!.id,
          chatId: "chat:foreign",
          currentLayout: oneChatLayout,
        },
        archivedLast.transitionEpoch,
      ),
    ).rejects.toThrow("does not belong");
    expect(await browserDesktop.getBootstrapSnapshot()).toEqual(
      beforeRejectedIdentities,
    );

    const fullLayout: ProjectLayoutRequest = {
      ...oneChatLayout,
      tabs: [
        ...oneChatLayout.tabs,
        ...Array.from({ length: 63 }, (_, index): WorkbenchTab => ({
          id: `terminal-${index + 1}`,
          kind: "terminal",
          title: `Terminal ${index + 1}`,
          position: index + 1,
          pinned: false,
          dirty: false,
          status: "idle",
        })),
      ],
    };
    await browserDesktop.persistProjectLayout(fullLayout);
    const beforePlusOne = await browserDesktop.getBootstrapSnapshot();
    await expect(
      browserDesktop.restoreChat(
        {
          projectId: initial.project!.id,
          chatId: secondChatId,
          currentLayout: fullLayout,
        },
        archivedLast.transitionEpoch,
      ),
    ).rejects.toThrow("at most 64 tabs");
    expect(await browserDesktop.getBootstrapSnapshot()).toEqual(beforePlusOne);
  });

  it("rejects stale foreign invalid and over-limit rename before mutation", async () => {
    const initial = await browserDesktop.getBootstrapSnapshot();
    const exact = "é".repeat(MAX_CHAT_TITLE_BYTES / 2);
    const renamed = await browserDesktop.renameChat(
      {
        projectId: initial.project!.id,
        chatId: initial.activeTabId!,
        title: exact,
        currentLayout: {
          projectId: initial.project!.id,
          tabs: initial.tabs,
          activeTabId: initial.activeTabId,
          selectedRootId: initial.selectedRootId,
          selectedResource: initial.selectedResource,
        },
      },
      initial.transitionEpoch,
    );
    expect(renamed.snapshot!.tabs[0].title).toBe(`Chat: ${exact}`);

    const currentLayout = {
      projectId: initial.project!.id,
      tabs: renamed.snapshot!.tabs,
      activeTabId: renamed.snapshot!.activeTabId,
      selectedRootId: renamed.snapshot!.selectedRootId,
      selectedResource: renamed.snapshot!.selectedResource,
    };
    const failedFile: WorkbenchTab = {
      id: "file:must-rollback",
      kind: "file",
      title: "must-rollback.ts",
      position: 1,
      pinned: false,
      dirty: true,
      status: "idle",
      resource: {
        rootId: initial.selectedRootId!,
        relativePath: "src/must-rollback.ts",
      },
    };
    const changedFailureLayout = {
      ...currentLayout,
      tabs: [...currentLayout.tabs, failedFile],
      activeTabId: failedFile.id,
      selectedResource: failedFile.resource!,
    };
    for (const [request, epoch] of [
      [
        {
          projectId: initial.project!.id,
          chatId: initial.activeTabId!,
          title: `${exact}x`,
          currentLayout: changedFailureLayout,
        },
        renamed.transitionEpoch,
      ],
      [
        {
          projectId: initial.project!.id,
          chatId: initial.activeTabId!,
          title: " \t ",
          currentLayout,
        },
        renamed.transitionEpoch,
      ],
      [
        {
          projectId: initial.project!.id,
          chatId: initial.activeTabId!,
          title: "bad\nname",
          currentLayout,
        },
        renamed.transitionEpoch,
      ],
      [
        {
          projectId: "foreign-project",
          chatId: initial.activeTabId!,
          title: "Foreign",
          currentLayout,
        },
        renamed.transitionEpoch,
      ],
      [
        {
          projectId: initial.project!.id,
          chatId: "foreign-chat",
          title: "Foreign",
          currentLayout,
        },
        renamed.transitionEpoch,
      ],
      [
        {
          projectId: initial.project!.id,
          chatId: initial.activeTabId!,
          title: "Stale",
          currentLayout,
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

  it("rejects native-invalid rename layouts before mutating browser state", async () => {
    const initial = await browserDesktop.getBootstrapSnapshot();
    const chat = initial.tabs[0];
    const rootId = initial.selectedRootId!;
    const validLayout: ProjectLayoutRequest = {
      projectId: initial.project!.id,
      tabs: initial.tabs,
      activeTabId: chat.id,
      selectedRootId: rootId,
      selectedResource: null,
    };
    const terminal = (id: string, position: number): WorkbenchTab => ({
      id,
      kind: "terminal",
      title: id,
      position,
      pinned: false,
      dirty: false,
    });
    const invalidLayouts: Array<[ProjectLayoutRequest, string]> = [
      [
        { ...validLayout, tabs: [chat, { ...terminal(chat.id, 1) }] },
        "duplicate tab identity or position",
      ],
      [
        { ...validLayout, tabs: [chat, terminal("terminal:duplicate", 0)] },
        "duplicate tab identity or position",
      ],
      [
        {
          ...validLayout,
          tabs: [chat, terminal("terminal:unsafe", 2 ** 53)],
        },
        "tab position must be a safe integer",
      ],
      [
        {
          ...validLayout,
          tabs: [
            chat,
            {
              ...terminal("terminal:pinned-string", 1),
              pinned: "false" as unknown as boolean,
            },
          ],
        },
        "tab pinned and dirty fields must be booleans",
      ],
      [
        {
          ...validLayout,
          tabs: [
            chat,
            {
              ...terminal("terminal:dirty-number", 1),
              dirty: 0 as unknown as boolean,
            },
          ],
        },
        "tab pinned and dirty fields must be booleans",
      ],
      [
        {
          ...validLayout,
          tabs: [
            chat,
            {
              ...terminal("terminal:oversized", 1),
              title: "x".repeat(2 * 1024 * 1024 + 1),
            },
          ],
        },
        "tab projection exceeds its metadata byte limit",
      ],
      [{ ...validLayout, activeTabId: "missing-tab" }, "active tab is absent"],
      [
        {
          ...validLayout,
          tabs: [
            chat,
            ...Array.from({ length: 64 }, (_, index) =>
              terminal(`terminal:${index}`, index + 1),
            ),
          ],
        },
        "at most 64 tabs",
      ],
      [
        {
          ...validLayout,
          tabs: [
            chat,
            {
              ...terminal("file:missing-resource", 1),
              kind: "file",
            },
          ],
        },
        "File and Diff tabs require a resource identity",
      ],
      [
        {
          ...validLayout,
          tabs: [
            chat,
            {
              ...terminal("diff:foreign-root", 1),
              kind: "diff",
              resource: { rootId: "foreign-root", relativePath: "a.ts" },
            },
          ],
        },
        "layout resource root does not belong",
      ],
      [
        {
          ...validLayout,
          tabs: [
            chat,
            {
              ...terminal("file:malformed-resource", 1),
              kind: "file",
              resource: {
                rootId: 7,
                relativePath: "a.ts",
              } as unknown as WorkbenchTab["resource"],
            },
          ],
        },
        "resource identity must use strings",
      ],
      [
        {
          ...validLayout,
          tabs: [
            {
              ...chat,
              resource: { rootId, relativePath: "a.ts" },
            },
          ],
        },
        "only File and Diff tabs may carry a resource",
      ],
      [
        {
          ...validLayout,
          tabs: [
            chat,
            {
              ...terminal("file:non-canonical", 1),
              kind: "file",
              resource: { rootId, relativePath: "src/../a.ts" },
            },
          ],
        },
        "canonical root-relative spelling",
      ],
      [
        {
          ...validLayout,
          tabs: [
            chat,
            {
              ...terminal("file:drive-relative", 1),
              kind: "file",
              resource: { rootId, relativePath: "C:a.ts" },
            },
          ],
        },
        "canonical root-relative spelling",
      ],
      [
        { ...validLayout, selectedRootId: "foreign-root" },
        "layout resource root does not belong",
      ],
      [
        {
          ...validLayout,
          selectedRootId: "foreign-root",
          selectedResource: {
            rootId: "foreign-root",
            relativePath: "a.ts",
          },
        },
        "layout resource root does not belong",
      ],
      [
        {
          ...validLayout,
          selectedResource: {
            rootId,
            relativePath: "src/../a.ts",
          },
        },
        "canonical root-relative spelling",
      ],
      [
        {
          ...validLayout,
          selectedRootId: null,
          selectedResource: { rootId, relativePath: "a.ts" },
        },
        "selected resource and selected root do not match",
      ],
    ];

    for (const [currentLayout, error] of invalidLayouts) {
      await expect(
        browserDesktop.renameChat(
          {
            projectId: initial.project!.id,
            chatId: chat.id,
            title: "Must not commit",
            currentLayout,
          },
          initial.transitionEpoch,
        ),
      ).rejects.toThrow(error);
      expect(await browserDesktop.getBootstrapSnapshot()).toEqual(initial);
    }
  });

  it("checks cheap rename identity before bounding layout and scanning tabs", async () => {
    const initial = await browserDesktop.getBootstrapSnapshot();
    const request = {
      projectId: initial.project!.id,
      chatId: initial.activeTabId!,
      title: "Must not commit",
      currentLayout: {
        projectId: initial.project!.id,
        tabs: new Proxy(initial.tabs, {
          get() {
            throw new Error("stale rename inspected its layout");
          },
        }),
        activeTabId: initial.activeTabId,
        selectedRootId: initial.selectedRootId,
        selectedResource: initial.selectedResource,
      },
    };
    await expect(
      browserDesktop.renameChat(request, initial.transitionEpoch + 1),
    ).rejects.toThrow("project changed before the requested Chat rename");

    const oversizedTabs = new Proxy(
      [
        initial.tabs[0],
        ...Array.from({ length: 64 }, (_, index): WorkbenchTab => ({
          id: `terminal:bounded:${index}`,
          kind: "terminal",
          title: "Terminal",
          position: index + 1,
          pinned: false,
          dirty: false,
        })),
      ],
      {
        get(target, property, receiver) {
          if (property === "some") {
            throw new Error("rename scanned tabs before bounding the layout");
          }
          return Reflect.get(target, property, receiver);
        },
      },
    );
    await expect(
      browserDesktop.renameChat(
        {
          ...request,
          currentLayout: { ...request.currentLayout, tabs: oversizedTabs },
        },
        initial.transitionEpoch,
      ),
    ).rejects.toThrow("project layout may contain at most 64 tabs");
    expect(await browserDesktop.getBootstrapSnapshot()).toEqual(initial);
  });

  it("bounds rename identifiers before durable lookup or layout access", () => {
    const neverHas = vi.fn(() => {
      throw new Error("rename hashed an unbounded Chat ID");
    });
    const durableChatIds = {
      has: neverHas,
    } as unknown as ReadonlySet<string>;
    const unreadableTabs = new Proxy([] as WorkbenchTab[], {
      get() {
        throw new Error(
          "rename inspected layout tabs during identity preflight",
        );
      },
    });
    const request = {
      projectId: "browser-project",
      chatId: "chat",
      title: "Must not commit",
      currentLayout: {
        projectId: "browser-project",
        tabs: unreadableTabs,
        activeTabId: null,
        selectedRootId: null,
        selectedResource: null,
      },
    };
    const validate = (
      projectId: string,
      chatId: string,
      ids: ReadonlySet<string> = durableChatIds,
    ) =>
      assertBrowserChatRenameIdentity(
        { ...request, projectId, chatId },
        0,
        0,
        "browser-project",
        ids,
      );

    for (const chatId of [
      "x".repeat(2 * 1024 * 1024),
      "",
      "\u00e9".repeat(257),
      "bad\nid",
    ]) {
      expect(() => validate("browser-project", chatId)).toThrow(
        "Chat ID is not a valid bounded Chat identifier",
      );
    }
    expect(neverHas).not.toHaveBeenCalled();

    const exact = "x".repeat(512);
    expect(() =>
      validate("browser-project", exact, new Set([exact])),
    ).not.toThrow();
    expect(() => validate("x".repeat(2 * 1024 * 1024), "chat")).toThrow(
      "Project ID is not a valid bounded Chat identifier",
    );
    expect(neverHas).not.toHaveBeenCalled();
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
