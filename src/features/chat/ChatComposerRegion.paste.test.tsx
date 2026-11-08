import {
  act,
  cleanup,
  createEvent,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { useMemo, type ComponentProps } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../../app/store";
import { MAX_CHAT_COMPOSER_BYTES } from "../../domain/chatOwnership";
import {
  PROMPT_ATTACHMENT_COUNT,
  PROMPT_ATTACHMENT_TOTAL_BYTES,
} from "../../ports/promptAttachments";
import { ChatComposerRegion } from "./ChatComposerRegion";
import { usePastedTextAttachments } from "./usePastedTextAttachments";

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
          throw "Attachments exceed 96 KiB total.";
        native.files.push({
          id: `file-${++native.next}`,
          name: "pasted-text.txt",
          bytes,
        });
        native.revision = `revision-${native.next}`;
        return summary();
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

function Composer({
  acceptsResources = true,
  ...overrides
}: Partial<Props> & { acceptsResources?: boolean | null }) {
  const pasted = usePastedTextAttachments("chat-a", null);
  const pastedText = useMemo(
    () => ({
      ...pasted,
      attachability: {
        nativeAcp: true,
        // No session yet means native admission decides at send time.
        session:
          acceptsResources === null
            ? null
            : { capabilities: { promptResources: acceptsResources } },
        queued: 0,
      },
    }),
    [pasted, acceptsResources],
  );
  const props: Props = {
    composerRef: { current: null },
    fieldId: "message",
    dictationScope: "chat-a",
    descriptionId: "description",
    disabledReasonId: "disabled-reason",
    text: "",
    attachedContext: [],
    pastedText,
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
  return <ChatComposerRegion {...props} />;
}

/** Fires a text paste; returns whether the browser would insert it. */
function paste(text: string) {
  const field = screen.getByRole("textbox", { name: "Message agent" });
  const event = createEvent.paste(field, {
    clipboardData: { files: [], getData: () => text },
  });
  fireEvent(field, event);
  return !event.defaultPrevented;
}

/** The composer's polite live region for paste and length notices. */
const status = () => document.querySelector<HTMLElement>("[aria-live=polite]")!;
/** The paste module and chips load lazily, so first use may take a moment. */
const LAZY = { timeout: 5000 };
const announced = (text: string) =>
  waitFor(() => expect(status()).toHaveTextContent(text), LAZY);
// 2,431 lines of 31 bytes: a log over the composer cap, inside the batch.
const log = `${"x".repeat(30)}\n`.repeat(2431);
const label = "Pasted text · 2,431 lines · 74 KB";

describe("composer large paste", () => {
  it("keeps a paste that fits the composer inline", async () => {
    const onTextChange = vi.fn();
    render(<Composer onTextChange={onTextChange} />);
    expect(paste("short note")).toBe(true);
    expect(native.calls).not.toContain("stage_prompt_text");
  });

  it("turns a paste over the cap into a previewable pasted-text chip", async () => {
    const onTextChange = vi.fn();
    render(<Composer text="Why does this fail?" onTextChange={onTextChange} />);
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

  it("allows sending staged pasted text with an empty message", async () => {
    render(<Composer />);
    const send = screen.getByRole("button", { name: "Send message" });
    expect(send).toBeDisabled();
    paste(log);
    await screen.findByRole("button", { name: label }, LAZY);
    expect(send).toBeEnabled();
  });

  it("removes the chip and its staged bytes", async () => {
    render(<Composer />);
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
    const huge = "y".repeat(PROMPT_ATTACHMENT_TOTAL_BYTES + 1);
    expect(paste(huge)).toBe(false);
    await announced(
      "Pasted text is 97 KB, over the 96 KB attachment limit. Nothing was pasted.",
    );
    expect(onTextChange).not.toHaveBeenCalled();
    expect(screen.getByRole("textbox", { name: "Message agent" })).toHaveValue(
      "Keep me",
    );
    expect(native.calls).not.toContain("stage_prompt_text");
  });

  it("refuses a second paste that no longer fits beside the first", async () => {
    render(<Composer />);
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
    expect(paste(log)).toBe(false);
    await announced(
      "This harness does not accept attached text (no embedded-context support), so text over 16 KB cannot be sent here. Nothing was pasted.",
    );
    expect(native.calls).not.toContain("stage_prompt_text");
  });

  it("reports a native refusal instead of dropping the paste", async () => {
    native.revision = "someone-else";
    render(<Composer />);
    paste(log);
    await announced(
      "Attachments changed; reload the staged attachments before retrying. Nothing was pasted.",
    );
  });

  it("holds back typing past the cap with an announced message", async () => {
    const onTextChange = vi.fn();
    const full = "a".repeat(MAX_CHAT_COMPOSER_BYTES);
    render(<Composer text={full} onTextChange={onTextChange} />);
    const field = screen.getByRole("textbox", { name: "Message agent" });
    expect(field).not.toHaveAttribute("maxlength");
    act(() => {
      fireEvent.change(field, { target: { value: `${full}é` } });
    });
    expect(onTextChange).not.toHaveBeenCalled();
    await announced(
      "Messages hold up to 16 KB of typed text; this edit would make 17 KB.",
    );
    fireEvent.change(field, { target: { value: full.slice(1) } });
    expect(onTextChange).toHaveBeenCalledWith(full.slice(1));
    expect(status()).toBeEmptyDOMElement();
  });
});
