import {
  act,
  cleanup,
  createEvent,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import type { ComponentProps } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../../app/store";
import { MAX_CHAT_COMPOSER_BYTES } from "../../domain/chatOwnership";
import {
  PROMPT_ATTACHMENT_COUNT,
  PROMPT_ATTACHMENT_TOTAL_BYTES,
} from "../../ports/promptAttachments";
import { ChatComposerRegion } from "./ChatComposerRegion";

/** In-memory stand-in for the Rust batch: same caps, same revision check. */
const native = vi.hoisted(() => ({
  revision: null as string | null,
  files: [] as { id: string; name: string; bytes: number }[],
  calls: [] as string[],
  next: 0,
}));
vi.mock(import("../../ports/nativeInvoke"), () => ({
  isTauriRuntime: () => true,
  invokeNative: vi.fn(
    async (command: string, args?: Record<string, unknown>) => {
      native.calls.push(command);
      const request = args?.request as
        | { expectedRevision?: string | null; text?: string; id?: string }
        | undefined;
      const summary = () =>
        native.files.length
          ? {
              revision: native.revision,
              files: native.files.map((f) => ({ ...f })),
            }
          : null;
      if (command === "list_prompt_files") return summary();
      if (command === "stage_prompt_text") {
        if (request?.expectedRevision !== native.revision)
          throw "Attachments changed; reload the staged attachments before retrying.";
        const bytes = new TextEncoder().encode(request?.text ?? "").length;
        const total = native.files.reduce((sum, f) => sum + f.bytes, 0);
        if (native.files.length >= PROMPT_ATTACHMENT_COUNT)
          throw "Attachments exceed the 16-file limit.";
        if (total + bytes > PROMPT_ATTACHMENT_TOTAL_BYTES)
          throw "Attachments exceed 96 KB total.";
        const id = `file-${++native.next}`;
        native.files.push({ id, name: "pasted-text.txt", bytes });
        native.revision = `revision-${native.next}`;
        // Native names the resource the paste added.
        return { ...summary(), added: id };
      }
      if (command === "remove_prompt_file") {
        native.files = native.files.filter((f) => f.id !== request?.id);
        native.revision = native.files.length
          ? `revision-${++native.next}`
          : null;
        return summary();
      }
      throw new Error(`unexpected ${command}`);
    },
  ) as never,
}));

const initial = useWorkbenchStore.getState();
beforeEach(() => {
  Object.assign(native, { revision: null, files: [], calls: [], next: 0 });
  useWorkbenchStore.setState({
    captureChatOwnership: (chatId = "chat-a") => ({
      projectId: "project-a",
      ownershipEpoch: 1,
      chatId,
      projectGeneration: 1,
      transitionEpoch: 3,
    }),
  });
});
afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initial, true);
});

type Props = ComponentProps<typeof ChatComposerRegion>;
/** The paste module and chips load lazily, so first use may take a moment. */
const LAZY = { timeout: 5000 };

/** The region under test; cold tests swap in a fresh, gated copy. */
let Region = ChatComposerRegion;
function Composer({
  acceptsResources = true,
  ...overrides
}: Partial<Props> & { acceptsResources?: boolean | null }) {
  const props: Props = {
    composerRef: { current: null },
    fieldId: "message",
    dictationScope: "chat-a",
    descriptionId: "description",
    disabledReasonId: "disabled-reason",
    text: "",
    attachedContext: [],
    // No session yet means native admission decides at send time.
    pasteSession:
      acceptsResources === null
        ? null
        : { capabilities: { promptResources: acceptsResources } },
    enabled: true,
    actionBusy: false,
    activeTurn: false,
    projectTransitioning: false,
    workbenchError: null,
    chatError: null,
    availableBinding: true,
    canPrompt: true,
    discoveryPhase: "ready",
    sessionHarnessId: null,
    sessionModelId: null,
    selectedHarnessId: null,
    selectedModelId: null,
    sessionPromptUnavailable: false,
    sessionConfig: null,
    modePending: false,
    modeError: null,
    focused: true,
    onSetSessionMode: vi.fn(),
    onSetSessionConfigOption: vi.fn(),
    onSelectHarness: vi.fn(),
    onSelectModel: vi.fn(),
    missingBindingNotice: false,
    disabledReason: null,
    project: { name: "Project" },
    usage: null,
    goal: { kind: "unsupported" },
    onSubmit: vi.fn((event) => event.preventDefault()),
    onTextChange: vi.fn(),
    onRemoveContext: vi.fn(),
    onCancelTurn: null,
    ...overrides,
  };
  return <Region {...props} />;
}

const field = () => screen.getByRole("combobox", { name: "Message agent" });

/** Waits for the paste part to load and read the staged batch. */
async function focusField() {
  await waitFor(
    () => expect(native.calls).toContain("list_prompt_files"),
    LAZY,
  );
}

/** Whether the field's aria-describedby takes in this element. */
const describesField = (element: HTMLElement) =>
  field()
    .getAttribute("aria-describedby")!
    .split(" ")
    .some((id) => document.getElementById(id)?.contains(element));

/** Fires a text paste; returns whether the browser would insert it. */
function paste(text: string, files: File[] = []) {
  const event = createEvent.paste(field(), {
    clipboardData: { files, getData: () => text },
  });
  fireEvent(field(), event);
  return !event.defaultPrevented;
}

/** The paste part's polite live region for paste and length notices; the
 * `/` menu keeps a live region of its own. */
const status = () =>
  document.querySelector<HTMLElement>(
    "#disabled-reason-paste[aria-live=polite]",
  );
const announced = (text: string) =>
  waitFor(() => expect(status()!).toHaveTextContent(text), LAZY);
// 2,431 lines of 31 bytes: a log over the composer cap, inside the batch.
const log = `${"x".repeat(30)}\n`.repeat(2431);
const label = "Pasted text · 2,431 lines · 74 KB";

// The first test pays for loading the lazy paste part.
describe("composer large paste", { timeout: 15_000 }, () => {
  it("outside a native chat, loads nothing for a paste that fits inline", async () => {
    render(<Composer availableBinding={false} />);
    fireEvent.focus(field());
    expect(paste("short note")).toBe(true);
    // Only held-back input loads the paste part there.
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(native.calls).toEqual([]);
  });

  it("leaves an image paste to the form's image input", async () => {
    render(<Composer />);
    await focusField();
    const image = new File(["x"], "shot.png", { type: "image/png" });
    paste(log, [image]);
    // The image input answered; no text was staged.
    expect(
      await screen.findByText(/Image input is unavailable/),
    ).toBeInTheDocument();
    expect(native.calls).not.toContain("stage_prompt_text");
  });

  it("stages a long paste made before the paste part loads", async () => {
    const onTextChange = vi.fn();
    render(<Composer onTextChange={onTextChange} />);
    // The part may not be loaded yet: the composer holds the paste back
    // synchronously and hands it over once the part arrives.
    expect(paste(log)).toBe(false);
    expect(
      await screen.findByRole("button", { name: label }, LAZY),
    ).toBeVisible();
    await announced(`Attached ${label}.`);
    expect(native.calls).toContain("stage_prompt_text");
    expect(onTextChange).not.toHaveBeenCalled();
  });
  it("holds back an over-cap edit that is not a paste, with a message for that path", async () => {
    const onTextChange = vi.fn();
    render(<Composer onTextChange={onTextChange} />);
    // Text dropped or typed past the cap never reaches the draft.
    fireEvent.change(field(), { target: { value: log } });
    expect(onTextChange).not.toHaveBeenCalled();
    await announced(
      "Messages hold up to 16 KB of typed text; this edit would make at least 74 KB. Paste longer text to attach it instead.",
    );
    expect(native.calls).not.toContain("stage_prompt_text");
  });
  it("does not suggest pasting where pasted text cannot attach", async () => {
    render(<Composer acceptsResources={false} />);
    fireEvent.change(field(), { target: { value: log } });
    await announced("This harness does not accept attached text");
    expect(status()!).not.toHaveTextContent("Paste longer text");
  });
  it("refuses a long paste while a send is in flight, and says so", async () => {
    render(<Composer actionBusy />);
    await focusField().catch(() => undefined);
    expect(paste(log)).toBe(false);
    await announced(
      "Wait for the message to finish sending, then paste again.",
    );
    expect(native.calls).not.toContain("stage_prompt_text");
  });
  it("holds a send until the batch staged before a reload is read back", async () => {
    native.files = [{ id: "file-9", name: "notes.md", bytes: 2048 }];
    native.revision = "revision-9";
    const onSubmit = vi.fn((event) => event.preventDefault());
    render(<Composer text="Draft from before" onSubmit={onSubmit} />);
    const send = screen.getByRole("button", { name: "Send message" });
    // A native chat loads the part at once, with no focus needed; until its
    // first read settles, no send goes, a click with no prior focus included.
    expect(send).toBeDisabled();
    await screen.findByText("notes.md · 2 KB", undefined, LAZY);
    expect(send).toBeEnabled();
    fireEvent.click(send);
    expect(onSubmit).toHaveBeenCalledWith(expect.anything(), "revision-9");
  });
  it("ties its notices to the field", async () => {
    render(<Composer />);
    await focusField();
    expect(describesField(status()!)).toBe(true);
    // Every element the field names exists.
    for (const id of field().getAttribute("aria-describedby")!.split(" "))
      expect(document.getElementById(id), id).not.toBeNull();
  });

  it("names no paste notices where the paste part never loaded", () => {
    render(<Composer availableBinding={false} />);
    expect(field().getAttribute("aria-describedby")).not.toContain("paste");
  });

  it("reads the staged batch when a native chat's composer mounts", async () => {
    native.files = [{ id: "file-9", name: "notes.md", bytes: 2048 }];
    native.revision = "revision-9";
    render(<Composer />);
    const send = screen.getByRole("button", { name: "Send message" });
    expect(send).toBeDisabled();
    await screen.findByText("notes.md · 2 KB", undefined, LAZY);
    expect(native.calls).toEqual(["list_prompt_files"]);
    // The batch alone can be sent, by its revision.
    expect(send).toBeEnabled();
  });

  it("turns a paste over the cap into a previewable pasted-text chip", async () => {
    const onTextChange = vi.fn();
    render(<Composer text="Why does this fail?" onTextChange={onTextChange} />);
    await focusField();
    expect(paste(log)).toBe(false);
    const chip = await screen.findByRole("button", { name: label }, LAZY);
    await announced(`Attached ${label}.`);
    expect(onTextChange).not.toHaveBeenCalled();
    expect(native.files).toEqual([
      { id: "file-1", name: "pasted-text.txt", bytes: log.length },
    ]);
    fireEvent.click(chip);
    expect(chip).toHaveAttribute("aria-expanded", "true");
    const preview = screen.getByRole("region", { name: `Preview of ${label}` });
    expect(preview.textContent).toBe(log);
  });

  it("sends staged pasted text, even alone, by its batch revision", async () => {
    const onSubmit = vi.fn((event: { preventDefault(): void }) =>
      event.preventDefault(),
    );
    render(<Composer onSubmit={onSubmit} />);
    const send = screen.getByRole("button", { name: "Send message" });
    expect(send).toBeDisabled();
    await focusField();
    paste(log);
    await screen.findByRole("button", { name: label }, LAZY);
    expect(send).toBeEnabled();
    fireEvent.click(send);
    expect(onSubmit).toHaveBeenCalledWith(expect.anything(), native.revision);
  });

  it("keeps staged pasted text out of a queued message", async () => {
    render(<Composer activeTurn queueAvailable />);
    await focusField();
    paste(log);
    await screen.findByRole("button", { name: label }, LAZY);
    expect(
      screen.getByRole("button", { name: /Queue message|Send message/ }),
    ).toBeDisabled();
    // A standing reason, apart from the transient live notices.
    const reason = screen.getByText(
      /Attached text goes with the next message you send, not a queued one\./,
    );
    expect(reason.closest("[aria-live]")).toBeNull();
    expect(describesField(reason)).toBe(true);
  });

  it("removes the chip and its staged bytes", async () => {
    render(<Composer />);
    await focusField();
    paste(log);
    await screen.findByRole("button", { name: label }, LAZY);
    fireEvent.click(screen.getByRole("button", { name: `Remove ${label}` }));
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: label })).toBeNull(),
    );
    expect(native.files).toEqual([]);
    await announced(`Removed ${label}.`);
  });

  it("refuses a paste beyond the attachment budget and leaves the draft untouched", async () => {
    const onTextChange = vi.fn();
    render(<Composer text="Keep me" onTextChange={onTextChange} />);
    await focusField();
    const huge = "y".repeat(PROMPT_ATTACHMENT_TOTAL_BYTES + 1);
    expect(paste(huge)).toBe(false);
    await announced(
      "Pasted text is at least 97 KB, over the 96 KB attachment limit. Nothing was pasted.",
    );
    expect(onTextChange).not.toHaveBeenCalled();
    expect(screen.getByRole("combobox", { name: "Message agent" })).toHaveValue(
      "Keep me",
    );
    expect(native.calls).not.toContain("stage_prompt_text");
  });

  it("refuses a second paste that no longer fits beside the first", async () => {
    render(<Composer />);
    await focusField();
    paste(log);
    await screen.findByRole("button", { name: label }, LAZY);
    expect(paste(log)).toBe(false);
    await announced(
      "Pasted text is 74 KB, but only 23 KB of the 96 KB attachment limit is left.",
    );
    expect(native.files).toHaveLength(1);
  });

  it("explains why a harness without embedded context cannot take the paste", async () => {
    render(<Composer acceptsResources={false} />);
    await focusField();
    expect(paste(log)).toBe(false);
    await announced(
      "This harness does not accept attached text (no embedded-context support), so text over 16 KB cannot be sent here. Nothing was pasted.",
    );
    expect(native.calls).not.toContain("stage_prompt_text");
  });

  it("reports a native refusal instead of dropping the paste", async () => {
    render(<Composer />);
    await focusField();
    native.revision = "someone-else";
    paste(log);
    await announced(
      "Attachments changed; reload the staged attachments before retrying. Nothing was pasted.",
    );
  });

  it("holds back typing past the cap with an announced message", async () => {
    const onTextChange = vi.fn();
    const full = "a".repeat(MAX_CHAT_COMPOSER_BYTES);
    const view = render(<Composer text={full} onTextChange={onTextChange} />);
    const field = screen.getByRole("combobox", { name: "Message agent" });
    expect(field).not.toHaveAttribute("maxlength");
    act(() => {
      fireEvent.change(field, { target: { value: `${full}é` } });
    });
    expect(onTextChange).not.toHaveBeenCalled();
    await announced(
      "Messages hold up to 16 KB of typed text; this edit would make at least 17 KB.",
    );
    fireEvent.change(field, { target: { value: full.slice(1) } });
    expect(onTextChange).toHaveBeenCalledWith(full.slice(1));
    view.rerender(
      <Composer text={full.slice(1)} onTextChange={onTextChange} />,
    );
    // The notice was about the earlier draft.
    expect(status()).toBeEmptyDOMElement();
  });
});

// The paste part's chunk arrives only when a test lets it, so pastes and
// edits made before then must be held, never dropped.
describe(
  "composer large paste before the paste part loads",
  { timeout: 15_000 },
  () => {
    let release = () => {};
    let fail = () => {};
    beforeEach(async () => {
      vi.resetModules();
      const gate = new Promise<void>((resolve, reject) => {
        release = resolve;
        fail = () => reject(new Error("The chunk did not load."));
      });
      vi.doMock("./ComposerPaste", async (importOriginal) => {
        await gate;
        return importOriginal();
      });
      Region = (await import("./ChatComposerRegion")).ChatComposerRegion;
      const fresh = (await import("../../app/store")).useWorkbenchStore;
      fresh.setState({
        captureChatOwnership: useWorkbenchStore.getState().captureChatOwnership,
      });
    });
    afterEach(() => {
      release();
      vi.doUnmock("./ComposerPaste");
      Region = ChatComposerRegion;
    });
    // 700 lines of 31 bytes: over the composer cap, and beside the log it
    // still fits the batch.
    const second = `${"y".repeat(30)}\n`.repeat(700);
    const secondLabel = "Pasted text · 700 lines · 22 KB";

    it("stages the first of two pastes made before it loads, and refuses the second aloud", async () => {
      render(<Composer />);
      expect(paste(log)).toBe(false);
      expect(paste(second)).toBe(false);
      await new Promise((resolve) => setTimeout(resolve, 50));
      // The chunk has not arrived: nothing was read or staged yet.
      expect(native.calls).toEqual([]);
      release();
      expect(
        await screen.findByRole("button", { name: label }, LAZY),
      ).toBeVisible();
      await announced(
        "Another long paste was still being attached. Nothing was pasted; paste again once it appears.",
      );
      expect(screen.queryByRole("button", { name: secondLabel })).toBeNull();
      expect(
        native.calls.filter((call) => call === "stage_prompt_text"),
      ).toHaveLength(1);
    });

    it("stages a paste followed by an over-cap edit, and answers both", async () => {
      const onTextChange = vi.fn();
      render(<Composer onTextChange={onTextChange} />);
      expect(paste(log)).toBe(false);
      fireEvent.change(field(), { target: { value: second } });
      expect(onTextChange).not.toHaveBeenCalled();
      await new Promise((resolve) => setTimeout(resolve, 50));
      expect(native.calls).toEqual([]);
      release();
      expect(
        await screen.findByRole("button", { name: label }, LAZY),
      ).toBeVisible();
      await announced("this edit would make at least 22 KB");
      expect(
        native.calls.filter((call) => call === "stage_prompt_text"),
      ).toHaveLength(1);
    });

    it("refuses a paste over the batch limit by its size, without keeping it", async () => {
      render(<Composer />);
      expect(paste("z".repeat(PROMPT_ATTACHMENT_TOTAL_BYTES + 1))).toBe(false);
      release();
      await announced(
        "Pasted text is at least 97 KB, over the 96 KB attachment limit.",
      );
      expect(native.calls).not.toContain("stage_prompt_text");
    });

    it("unblocks sends and says so when the part cannot load", async () => {
      render(<Composer text="Draft from before" />);
      const send = screen.getByRole("button", { name: "Send message" });
      expect(send).toBeDisabled();
      expect(paste(log)).toBe(false);
      fail();
      const notice = await screen.findByText(
        /Text over the message limit cannot be attached/,
        undefined,
        LAZY,
      );
      expect(notice).toBeVisible();
      expect(describesField(notice)).toBe(true);
      await waitFor(() => expect(send).toBeEnabled(), LAZY);
      expect(native.calls).toEqual([]);
      // A later long paste is still never inserted.
      expect(paste(log)).toBe(false);
    });

    it("loads the part again on a later mount after a failed load", async () => {
      const view = render(<Composer />);
      fail();
      const notice = await screen.findByText(
        /Text over the message limit cannot be attached/,
        undefined,
        LAZY,
      );
      // The failed mount keeps its notice across renders, without loading
      // again on each one.
      view.rerender(<Composer text="Edited" />);
      view.rerender(<Composer text="Edited again" />);
      await new Promise((resolve) => setTimeout(resolve, 50));
      expect(notice.isConnected).toBe(true);
      view.unmount();
      // The chunk is reachable again; the failure was not kept.
      vi.doMock("./ComposerPaste", (importOriginal) => importOriginal());
      native.files = [{ id: "file-9", name: "notes.md", bytes: 2048 }];
      native.revision = "revision-9";
      render(<Composer />);
      expect(
        await screen.findByText("notes.md · 2 KB", undefined, LAZY),
      ).toBeVisible();
      expect(
        screen.queryByText(/Text over the message limit cannot be attached/),
      ).toBeNull();
      expect(paste(log)).toBe(false);
      expect(
        await screen.findByRole("button", { name: label }, LAZY),
      ).toBeVisible();
    });

    it("names only notice elements that exist, while it loads too", async () => {
      render(<Composer />);
      const named = () =>
        field().getAttribute("aria-describedby")!.split(" ").filter(Boolean);
      expect(named()).toContain("disabled-reason-paste");
      for (const id of named())
        expect(document.getElementById(id), id).not.toBeNull();
      release();
      await focusField();
      for (const id of named())
        expect(document.getElementById(id), id).not.toBeNull();
    });
  },
);
