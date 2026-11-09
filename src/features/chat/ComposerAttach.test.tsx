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
import {
  agentSessionFixture,
  capabilitySetFixture,
} from "../../domain/acpProjection.fixture";
import { ChatComposerRegion } from "./ChatComposerRegion";

const native = vi.hoisted(() => vi.fn());
vi.mock("../../ports/nativeInvoke", async (importOriginal) => ({
  ...((await importOriginal()) as object),
  invokeNative: native,
  isTauriRuntime: () => true,
}));

const initial = useWorkbenchStore.getState();
/** The batch's owner loads lazily, so a pick may take a moment to land. */
const LAZY = { timeout: 5000 };
const owner = {
  projectId: "project-a",
  chatId: "chat-a",
  projectGeneration: 3,
};
const staged = (revision: string, names: string[]) => ({
  revision,
  files: names.map((name, index) => ({ id: `f${index}`, name, bytes: 2048 })),
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

describe("composer attach button", () => {
  it("stages picked files natively and removes them by revision", async () => {
    withSession(true);
    native.mockImplementation(async (command: string) => {
      if (command === "list_prompt_files") return staged("r1", ["old.md"]);
      if (command === "stage_prompt_files")
        return staged("r2", ["old.md", "notes.txt"]);
      if (command === "remove_prompt_file") return staged("r3", ["notes.txt"]);
      throw new Error(command);
    });
    renderComposer();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Attach files" }));
    });
    // Picked files join the chat's one batch, beside any pasted text. Its
    // owner loads lazily, so the pick lands after the batch read back.
    expect(
      await screen.findByText("notes.txt · 2 KB", undefined, LAZY),
    ).toBeInTheDocument();
    const chips = screen.getByRole("list", {
      name: "Attachments for the next message",
    });
    // The picker replaces the batch read back first, never a stale one.
    expect(native.mock.calls.map(([command]) => command)).toEqual([
      "list_prompt_files",
      "stage_prompt_files",
    ]);
    expect(native).toHaveBeenCalledWith("stage_prompt_files", {
      request: { owner, pick: "files", expectedRevision: "r1" },
    });
    expect(within(chips).getAllByRole("listitem")).toHaveLength(2);
    // The batch's owner announces it, in the notices the field names.
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
      command === "stage_prompt_files" ? staged("r1", ["a.md"]) : null,
    );
    const onSubmit = vi.fn((event: { preventDefault(): void }) =>
      event.preventDefault(),
    );
    renderComposer({ onSubmit });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Attach files" }));
    });
    expect(
      await screen.findByText("a.md · 2 KB", undefined, LAZY),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    expect(onSubmit).toHaveBeenCalledWith(expect.anything(), "r1");
  });

  it("shows the batch native already holds for the chat without a press", async () => {
    withSession(true);
    // As after a reload: native kept the batch the WebView forgot.
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
      command === "stage_prompt_files" ? staged("r1", ["a.md"]) : null,
    );
    const view = renderComposer();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Attach files" }));
    });
    expect(await screen.findByText("a.md · 2 KB")).toBeInTheDocument();
    await act(async () => {
      view.rerender(composer({ dictationScope: "chat-b" }));
    });
    expect(screen.queryByText("a.md · 2 KB")).toBeNull();
    expect(
      native.mock.calls.filter(([command]) => command === "stage_prompt_files"),
    ).toHaveLength(1);
  });

  it("refuses without opening the picker when the harness takes no files", async () => {
    withSession(false);
    native.mockResolvedValue(null);
    renderComposer();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Attach files" }));
    });
    expect(
      await screen.findByText(/does not accept attached files/u),
    ).toBeInTheDocument();
    expect(native).not.toHaveBeenCalledWith(
      "stage_prompt_files",
      expect.anything(),
    );
  });

  it("offers the image path beside files when the harness takes images", async () => {
    withSession(true);
    native.mockResolvedValue(null);
    const onAddImages = vi.fn();
    renderComposer({ imageInputAvailable: true, onAddImages });
    const chooser = screen.getByLabelText<HTMLInputElement>("Choose images");
    const click = vi.spyOn(chooser, "click");
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Attach files" }));
    });
    await act(async () => {
      fireEvent.click(await screen.findByRole("menuitem", { name: "Images…" }));
    });
    expect(click).toHaveBeenCalledOnce();
    expect(native).not.toHaveBeenCalledWith(
      "stage_prompt_files",
      expect.anything(),
    );
  });
});
