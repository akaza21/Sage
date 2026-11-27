import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import type { ComponentProps } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  acpProjectionStoreFor,
  clearAcpProjectionRegistry,
} from "../../app/acpProjectionRegistry";
import { useWorkbenchStore } from "../../app/store";
import { decodeSessionConfig } from "../../domain/acpSessionConfig";
import {
  agentSessionFixture,
  capabilitySetFixture,
} from "../../domain/acpProjection.fixture";
import { settleLazyModules } from "../../test/settleLazyModules";
import { ChatComposerRegion } from "./ChatComposerRegion";

const native = vi.hoisted(() => vi.fn());
vi.mock("../../ports/nativeInvoke", async (importOriginal) => ({
  ...((await importOriginal()) as object),
  invokeNative: native,
  isTauriRuntime: () => true,
}));

const initial = useWorkbenchStore.getState();
const owner = {
  projectId: "project-a",
  chatId: "chat-a",
  projectGeneration: 3,
};
const staged = (revision: string, names: string[]) => ({
  revision,
  files: names.map((name, index) => ({ id: `f${index}`, name, bytes: 2048 })),
});
const picked = (revision: string, names: string[]) => ({
  files: staged(revision, names),
});

function withSession(promptResources: boolean) {
  acpProjectionStoreFor("chat-a").setState({
    page: {
      currentSessionId: "session-a",
      sessions: [
        agentSessionFixture({
          id: "session-a" as never,
          capabilities: capabilitySetFixture({ promptResources }),
        }),
      ],
    } as never,
  });
}

function composer(
  overrides: Partial<ComponentProps<typeof ChatComposerRegion>> = {},
) {
  return (
    <ChatComposerRegion
      composerRef={{ current: null }}
      fieldId="message"
      dictationScope="chat-a"
      descriptionId="description"
      disabledReasonId="disabled-reason"
      text="Review these"
      attachedContext={[]}
      enabled
      actionBusy={false}
      activeTurn={false}
      projectTransitioning={false}
      workbenchError={null}
      chatError={null}
      availableBinding
      canPrompt
      discoveryPhase="ready"
      sessionHarnessId={null}
      sessionModelId={null}
      selectedHarnessId={null}
      selectedModelId={null}
      sessionPromptUnavailable={false}
      sessionConfig={null}
      modePending={false}
      modeError={null}
      focused
      onSetSessionMode={vi.fn()}
      onSetSessionConfigOption={vi.fn()}
      onSelectHarness={vi.fn()}
      onSelectModel={vi.fn()}
      missingBindingNotice={false}
      disabledReason={null}
      project={{ name: "Project" }}
      usage={null}
      goal={{ kind: "unsupported" }}
      onSubmit={vi.fn()}
      onTextChange={vi.fn()}
      onRemoveContext={vi.fn()}
      onCancelTurn={null}
      {...overrides}
    />
  );
}

function renderComposer(
  overrides: Partial<ComponentProps<typeof ChatComposerRegion>> = {},
) {
  return render(composer(overrides));
}

beforeEach(() => {
  useWorkbenchStore.setState({
    captureChatOwnership: (chatId: string) =>
      chatId === "chat-a"
        ? ({ ...owner, transitionEpoch: 3, ownershipEpoch: 1 } as never)
        : null,
  } as never);
});
afterEach(() => {
  cleanup();
  native.mockReset();
  clearAcpProjectionRegistry();
  useWorkbenchStore.setState(initial, true);
});

async function openAttach() {
  fireEvent.click(screen.getByRole("button", { name: "Composer options" }));
  await settleLazyModules();
  await act(async () => {
    fireEvent.click(screen.getByRole("menuitem", { name: "Attach" }));
  });
  await settleLazyModules();
}

async function pick(name: "Files…" | "Folder…") {
  await openAttach();
  await act(async () => {
    fireEvent.click(screen.getByRole("menuitem", { name }));
  });
}

describe("composer attach from the + menu", () => {
  it("stages picked files natively and removes them by revision", async () => {
    withSession(true);
    native.mockImplementation(async (command: string) => {
      if (command === "list_prompt_files") return staged("r1", ["old.md"]);
      if (command === "stage_prompt_files")
        return picked("r2", ["old.md", "notes.txt"]);
      if (command === "remove_prompt_file") return staged("r3", ["notes.txt"]);
      throw new Error(command);
    });
    renderComposer();
    await pick("Files…");
    await settleLazyModules();
    expect(await screen.findByText("notes.txt · 2 KB")).toBeInTheDocument();
    const chips = screen.getByRole("list", {
      name: "Attachments for the next message",
    });
    expect(native.mock.calls.map(([command]) => command)).toEqual([
      "list_prompt_files",
      "stage_prompt_files",
    ]);
    expect(native).toHaveBeenCalledWith("stage_prompt_files", {
      request: { owner, pick: "files", images: false, expectedRevision: "r1" },
    });
    expect(within(chips).getAllByRole("listitem")).toHaveLength(2);
    expect(
      document.querySelector("#disabled-reason-paste [aria-live=polite]"),
    ).toHaveTextContent("2 files attached to the next message.");
    await act(async () => {
      fireEvent.click(
        screen.getByRole("button", { name: "Remove old.md · 2 KB" }),
      );
    });
    expect(native).toHaveBeenLastCalledWith("remove_prompt_file", {
      request: { owner, revision: "r2", id: "f0" },
    });
    expect(within(chips).getAllByRole("listitem")).toHaveLength(1);
  });

  it("sends picked files by the batch revision the send names", async () => {
    withSession(true);
    native.mockImplementation(async (command: string) =>
      command === "stage_prompt_files" ? picked("r1", ["a.md"]) : null,
    );
    const onSubmit = vi.fn((event: { preventDefault(): void }) =>
      event.preventDefault(),
    );
    renderComposer({ onSubmit });
    await pick("Files…");
    await settleLazyModules();
    expect(await screen.findByText("a.md · 2 KB")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    expect(onSubmit).toHaveBeenCalledWith(expect.anything(), "r1");
  });

  it("stages a folder picked in native's folder dialog", async () => {
    withSession(true);
    native.mockImplementation(async (command: string) =>
      command === "stage_prompt_files" ? picked("r1", ["src/a.ts"]) : null,
    );
    renderComposer();
    await pick("Folder…");
    expect(await screen.findByText("src/a.ts · 2 KB")).toBeInTheDocument();
    expect(native).toHaveBeenCalledWith("stage_prompt_files", {
      request: { owner, pick: "folder", images: false, expectedRevision: null },
    });
  });

  it("shows the batch native already holds for the chat without a press", async () => {
    withSession(true);
    native.mockImplementation(async (command: string) =>
      command === "list_prompt_files" ? staged("r1", ["kept.md"]) : null,
    );
    renderComposer();
    expect(await screen.findByText("kept.md · 2 KB")).toBeInTheDocument();
    expect(native.mock.calls).toEqual([["list_prompt_files", { owner }]]);
  });

  it("starts another chat from its own batch without reopening the picker", async () => {
    withSession(true);
    native.mockImplementation(async (command: string) =>
      command === "stage_prompt_files" ? picked("r1", ["a.md"]) : null,
    );
    const view = renderComposer();
    await pick("Files…");
    expect(await screen.findByText("a.md · 2 KB")).toBeInTheDocument();
    await act(async () => {
      view.rerender(composer({ dictationScope: "chat-b" }));
    });
    expect(screen.queryByText("a.md · 2 KB")).toBeNull();
    expect(
      native.mock.calls.filter(([command]) => command === "stage_prompt_files"),
    ).toHaveLength(1);
  });

  it("shows Attach disabled with its reason when the harness takes neither files nor images", async () => {
    withSession(false);
    native.mockResolvedValue(null);
    renderComposer();
    await openAttach();
    const row = screen.getByRole("menuitem", { name: "Attach" });
    expect(row).toHaveAttribute("aria-disabled", "true");
    expect(row).toHaveAccessibleDescription(
      "This harness does not accept attached files (no embedded-context support).",
    );
    expect(row).toHaveTextContent(/does not accept attached files/u);
    // The press is refused in place: the "+" menu stays, no picker opens.
    expect(
      screen.getByRole("menu", { name: "Composer options menu" }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("menu", { name: "Attach" })).toBeNull();
    expect(native).not.toHaveBeenCalledWith(
      "stage_prompt_files",
      expect.anything(),
    );
  });

  it("keeps attaching open for a queued follow-up while a turn runs", async () => {
    withSession(true);
    native.mockResolvedValue(null);
    renderComposer({
      activeTurn: true,
      queueAvailable: true,
      imageInputAvailable: true,
      onAddImages: vi.fn(),
      sessionConfig: decodeSessionConfig({
        sageAgentSessionId: "session-a",
        runtimeEpoch: "1",
        configRevision: "1",
        modes: {
          currentModeId: "default",
          availableModes: [
            { id: "default", name: "Default" },
            { id: "review", name: "Review" },
          ],
        },
        configOptions: [],
      }),
    });
    expect(
      screen.getByRole("button", { name: "Composer options" }),
    ).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "Composer options" }));
    await settleLazyModules();
    // Changing the running session waits; attaching does not.
    expect(
      screen.getByRole("menuitemradio", { name: "Review" }),
    ).toBeDisabled();
    await act(async () => {
      fireEvent.click(screen.getByRole("menuitem", { name: "Attach" }));
    });
    await settleLazyModules();
    const menu = screen.getByRole("menu", { name: "Attach" });
    expect(
      within(menu).getByRole("menuitem", { name: "Files…" }),
    ).toBeEnabled();
  });

  it("replaces the attach menu with the + menu when + is pressed again", async () => {
    withSession(true);
    native.mockResolvedValue(null);
    renderComposer();
    await openAttach();
    expect(screen.getByRole("menu", { name: "Attach" })).toBeInTheDocument();
    const plus = screen.getByRole("button", { name: "Composer options" });
    fireEvent.pointerDown(plus);
    fireEvent.click(plus);
    expect(
      await screen.findByRole("menu", { name: "Composer options menu" }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("menu", { name: "Attach" })).toBeNull();
  });

  it("offers only Files… and Folder…, with no separate image item", async () => {
    withSession(true);
    native.mockResolvedValue(null);
    renderComposer({ imageInputAvailable: true, onAddImages: vi.fn() });
    await openAttach();
    const menu = screen.getByRole("menu", { name: "Attach" });
    expect(
      within(menu)
        .getAllByRole("menuitem")
        .map((item) => item.textContent),
    ).toEqual(["Files…", "Folder…"]);
  });

  it("attaches an image picked through Files… as image content", async () => {
    withSession(true);
    const image = {
      kind: "image" as const,
      artifactId: "11111111-1111-4111-8111-111111111111",
      mediaType: "image/png" as const,
      width: 1,
      height: 1,
      byteLength: 68,
      sha256: "a".repeat(64),
      label: "shot.png",
    };
    native.mockImplementation(async (command: string) =>
      command === "stage_prompt_files"
        ? { files: null, images: [image] }
        : null,
    );
    const attached: unknown[] = [];
    useWorkbenchStore.setState({
      attachChatContext: (intent: unknown, context: unknown) =>
        attached.push([intent, context]) > 0,
    } as never);
    const onAddImages = vi.fn();
    renderComposer({ imageInputAvailable: true, onAddImages });
    await pick("Files…");
    await settleLazyModules();
    expect(native).toHaveBeenCalledWith("stage_prompt_files", {
      request: { owner, pick: "files", images: true, expectedRevision: null },
    });
    expect(attached).toEqual([
      [
        expect.objectContaining({ chatId: "chat-a", transitionEpoch: 3 }),
        image,
      ],
    ]);
    // Native already stored it: nothing is uploaded again or staged as a file.
    expect(onAddImages).not.toHaveBeenCalled();
    expect(screen.queryByText(/shot\.png ·/u)).toBeNull();
    expect(
      document.querySelector("#disabled-reason-paste [aria-live=polite]"),
    ).toHaveTextContent("1 file attached to the next message.");
  });

  it("removes every picked image again when they would put the chat over 4", async () => {
    withSession(true);
    const image = (index: number) => ({
      kind: "image" as const,
      artifactId: `11111111-1111-4111-8111-11111111111${index}`,
      mediaType: "image/png" as const,
      width: 1,
      height: 1,
      byteLength: 68,
      sha256: "a".repeat(64),
      label: `shot-${index}.png`,
    });
    const held = [0, 1, 2].map(image);
    native.mockImplementation(async (command: string) =>
      command === "stage_prompt_files"
        ? { files: null, images: [image(3), image(4)] }
        : null,
    );
    const attach = vi.fn(() => true);
    useWorkbenchStore.setState({
      chatUiById: { "chat-a": { attachedContext: held } },
      attachChatContext: attach,
    } as never);
    renderComposer({
      imageInputAvailable: true,
      onAddImages: vi.fn(),
      attachedContext: held,
    });
    await pick("Files…");
    await settleLazyModules();
    expect(attach).not.toHaveBeenCalled();
    const removed = native.mock.calls
      .filter(([command]) => command === "remove_chat_image")
      .map(([, { request }]) => request.artifactId);
    expect(removed).toEqual([image(3).artifactId, image(4).artifactId]);
    expect(
      document.querySelector("#disabled-reason-paste [aria-live=polite]"),
    ).toHaveTextContent("A message can include at most 4 images.");
  });

  it("stages picked images as files once the chat already holds 4", async () => {
    withSession(true);
    native.mockImplementation(async (command: string) =>
      command === "stage_prompt_files" ? picked("r1", ["shot.png"]) : null,
    );
    const held = [0, 1, 2, 3].map((index) => ({
      kind: "image" as const,
      artifactId: `00000000-0000-4000-8000-00000000000${index}`,
      mediaType: "image/png" as const,
      width: 1,
      height: 1,
      byteLength: 1,
      sha256: "a".repeat(64),
      label: `image-${index}.png`,
    }));
    renderComposer({
      imageInputAvailable: true,
      onAddImages: vi.fn(),
      attachedContext: held,
    });
    await pick("Files…");
    expect(native).toHaveBeenCalledWith("stage_prompt_files", {
      request: { owner, pick: "files", images: false, expectedRevision: null },
    });
  });

  it("picks images only through Files… where the harness takes images but no files", async () => {
    withSession(false);
    native.mockResolvedValue(null);
    renderComposer({ imageInputAvailable: true, onAddImages: vi.fn() });
    await openAttach();
    const chooser = screen.getByLabelText<HTMLInputElement>("Choose images");
    const click = vi.spyOn(chooser, "click");
    expect(screen.getByRole("menuitem", { name: "Folder…" })).toBeDisabled();
    await act(async () => {
      fireEvent.click(screen.getByRole("menuitem", { name: "Files…" }));
    });
    expect(click).toHaveBeenCalledOnce();
    expect(native).not.toHaveBeenCalledWith(
      "stage_prompt_files",
      expect.anything(),
    );
  });
});

describe("composer attach in a chat with no session yet", () => {
  it("opens a visible menu whose Files item asks native for the file pick", async () => {
    // A new chat: images are a draft action, so the press opens the menu.
    acpProjectionStoreFor("chat-a").setState({
      page: { currentSessionId: null, sessions: [] } as never,
    });
    native.mockImplementation(async (command: string) =>
      command === "stage_prompt_files" ? picked("r1", ["a.md"]) : null,
    );
    renderComposer({ imageInputAvailable: true, onAddImages: vi.fn() });
    await openAttach();
    const menu = await screen.findByRole("menu", { name: "Attach" });
    expect(
      menu.closest<HTMLElement>(".menu-popover")?.style.visibility,
    ).not.toBe("hidden");
    await act(async () => {
      fireEvent.click(within(menu).getByRole("menuitem", { name: /^Files/u }));
    });
    expect(await screen.findByText("a.md · 2 KB")).toBeInTheDocument();
    expect(native).toHaveBeenCalledWith("stage_prompt_files", {
      request: { owner, pick: "files", images: true, expectedRevision: null },
    });
  });
});
