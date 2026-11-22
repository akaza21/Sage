import { composerProblem } from "./ComposerStatusNotices";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ComponentProps } from "react";

import { ChatComposerRegion } from "./ChatComposerRegion";
import { settleLazyModules } from "../../test/settleLazyModules";
import { useWorkbenchStore } from "../../app/store";
import {
  acpProjectionStoreFor,
  clearAcpProjectionRegistry,
} from "../../app/acpProjectionRegistry";
import {
  agentSessionFixture,
  capabilitySetFixture,
} from "../../domain/acpProjection.fixture";

const png = () => new File(["png"], "sample.png", { type: "image/png" });
const initial = useWorkbenchStore.getState();
afterEach(() => {
  cleanup();
  clearAcpProjectionRegistry();
  useWorkbenchStore.setState(initial, true);
});

function renderComposer(
  overrides: Partial<ComponentProps<typeof ChatComposerRegion>> = {},
) {
  const onAddImages = vi.fn();
  const props: ComponentProps<typeof ChatComposerRegion> = {
    composerRef: { current: null },
    fieldId: "message",
    dictationScope: "chat-a",
    descriptionId: "description",
    disabledReasonId: "disabled-reason",
    text: "A prompt",
    attachedContext: [],
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
    onAddImages,
    ...overrides,
  };
  const view = render(<ChatComposerRegion {...props} />);
  return {
    onAddImages,
    ...view,
    rerenderComposer: (next: Partial<typeof props>) =>
      view.rerender(<ChatComposerRegion {...props} {...next} />),
  };
}

async function openAttachMenu() {
  fireEvent.click(screen.getByRole("button", { name: "Composer options" }));
  await settleLazyModules();
  fireEvent.click(screen.getByRole("menuitem", { name: "Files and folders" }));
  await settleLazyModules();
}

it("locks the branch chip for this composer's turn and submission, before and after opening its menu", async () => {
  useWorkbenchStore.setState({
    project: {
      id: "project-a",
      name: "Alpha",
      roots: [
        {
          id: "root-a",
          name: "Alpha",
          ordinal: 0,
          availability: "available",
          message: null,
        },
      ],
    },
    repositories: {
      roots: [
        {
          rootId: "root-a",
          state: "repository",
          branch: "main",
          branches: ["main", "topic"],
          changes: [],
          truncated: false,
          elapsedMs: 1,
          message: null,
        },
      ],
      truncated: false,
      elapsedMs: 1,
    },
  });
  const { rerenderComposer } = renderComposer({ newChat: true });
  const chip = () =>
    screen.getByRole("button", { name: "Switch branch in Alpha" });
  await screen.findByRole("button", { name: "Switch branch in Alpha" });
  for (const menuLoaded of [false, true]) {
    expect(chip()).toBeEnabled();
    if (menuLoaded) {
      fireEvent.click(chip());
      await screen.findByRole("listbox", { name: "Switch branch in Alpha" });
    }
    rerenderComposer({ activeTurn: true });
    expect(chip()).toBeDisabled();
    expect(chip()).toHaveAttribute(
      "title",
      "Can't switch branches while the agent is working",
    );
    expect(screen.queryByRole("listbox")).toBeNull();
    rerenderComposer({ actionBusy: true });
    expect(chip()).toBeDisabled();
    fireEvent.click(chip());
    expect(screen.queryByRole("listbox")).toBeNull();
    rerenderComposer({ activeTurn: false, actionBusy: false });
    expect(chip()).toBeEnabled();
  }
});

describe("composer image input", () => {
  it("resolves one image problem reported by workspace, send and attachment channels at the attachment", () => {
    renderComposer({
      workbenchError: composerProblem(
        "image-input",
        "chat-a",
        "Image could not be added.",
      ),
      chatError: composerProblem(
        "image-input",
        "chat-a",
        "Cannot send the image.",
      ),
      imageInputDisabledReason: "Image could not be added.",
    });
    fireEvent.drop(screen.getByRole("form"), {
      dataTransfer: { files: [png()], types: ["Files"] },
    });
    expect(screen.getAllByRole("alert")).toHaveLength(1);
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Image could not be added.",
    );
    expect(screen.queryByText("Cannot send the image.")).toBeNull();
  });
  it("unmounts hidden toolbar controls while preserving the draft", () => {
    const { container, rerenderComposer } = renderComposer();
    expect(screen.getByRole("button", { name: "Harness" })).toBeVisible();
    rerenderComposer({
      hidden: true,
      usage: {
        inputTokens: 1000,
        outputTokens: 0,
        totalTokens: 1000,
        contextWindow: 272000,
      },
    });
    expect(container.querySelector(".composer__context")).toBeNull();
    expect(container.querySelector(".composer__controls")).toBeNull();
    rerenderComposer({ hidden: false });
    expect(screen.getByRole("button", { name: "Harness" })).toBeVisible();
    expect(screen.getByRole("combobox", { name: "Message agent" })).toHaveValue(
      "A prompt",
    );
  });
  it("leaves prompt paths and code uncorrected", () => {
    renderComposer({ text: "src/loong_file.ts npm --flag" });
    const prompt = screen.getByRole("combobox", { name: "Message agent" });
    expect(prompt).toHaveAttribute("spellcheck", "false");
    expect(prompt).toHaveAttribute("autocorrect", "off");
    expect(prompt).toHaveAttribute("autocapitalize", "off");
    expect(prompt).toHaveValue("src/loong_file.ts npm --flag");
  });
  it("keeps the draft focused while sending and saving a queued message", () => {
    const { rerenderComposer } = renderComposer();
    const prompt = screen.getByRole("combobox", { name: "Message agent" });
    prompt.focus();
    rerenderComposer({ actionBusy: true });
    expect(prompt).toBeEnabled();
    expect(prompt).toHaveFocus();
    expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
    rerenderComposer({ activeTurn: true, queueAvailable: false });
    expect(prompt).toBeEnabled();
    expect(prompt).toHaveFocus();
  });

  it.each(["Claude", "Codex", "Pi", "OpenCode", "OMP"])(
    "omits Goal actions for %s without admitted mutation capabilities",
    async (name) => {
      useWorkbenchStore.setState({
        harnesses: [
          {
            id: "fixture",
            name,
            description: "",
            installState: "available",
            source: "builtIn",
            transport: "acp",
            bindingId: "fixture",
            accountModelDiscovery: false,
          },
        ],
      });
      renderComposer({
        selectedHarnessId: "fixture",
        sessionHarnessId: "fixture",
      });
      expect(screen.queryByText("Goal", { exact: true })).toBeNull();
      const options = screen.queryByRole("button", {
        name: "Composer options",
      });
      if (options && !options.hasAttribute("disabled")) {
        fireEvent.click(options);
        await screen.findByRole("menu", { name: "Composer options menu" });
      }
      expect(screen.queryByRole("menuitem", { name: "Goal" })).toBeNull();
      expect(
        screen.getByRole("menuitem", { name: "Files and folders" }),
      ).toHaveAttribute("aria-disabled", "true");
    },
  );

  it("keeps the running prompt plain while describing queue behavior accessibly", () => {
    renderComposer({ activeTurn: true, queueAvailable: true });
    const prompt = screen.getByRole("combobox", { name: "Message agent" });
    expect(prompt).toHaveAttribute("placeholder", "Do anything");
    expect(prompt).toHaveAccessibleDescription(/Enter queues a message/);
  });

  it("gates only Add Goal by binding and picker identity, preserving native details actions", async () => {
    const goal = {
      kind: "goal" as const,
      snapshot: {
        objective: "Ship safely",
        status: "active" as const,
        tokenBudget: null,
        tokensUsed: 0,
        timeUsedSeconds: 1,
        createdAt: null,
        updatedAt: null,
      },
    };
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      projectGeneration: 0,
      projectTransitioning: false,
      activeTabId: "chat-a",
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "A",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
    });
    acpProjectionStoreFor("chat-a").setState({
      scope: {
        projectId: "project-a",
        projectGeneration: "0",
        chatId: "chat-a",
      } as never,
      page: {
        projectId: "project-a",
        chatId: "chat-a",
        title: "A",
        projectionRevision: "1",
        nextCursor: null,
        turns: [],
        goal,
        currentSessionId: "session-a",
        sessions: [
          agentSessionFixture({
            id: "session-a" as never,
            runtimeEpoch: "1" as never,
            capabilities: capabilitySetFixture({
              protocolVersion: "codex-app-server-observed-0.167.0",
              goal: {
                observes: true,
                set: false,
                pause: true,
                resume: false,
                tokenBudget: false,
                clear: true,
              },
            }),
          }),
        ],
      } as never,
    });
    const { rerenderComposer } = renderComposer({
      goal,
      goalCapabilities: {
        observes: true,
        set: false,
        pause: true,
        resume: false,
        tokenBudget: false,
        clear: true,
      },
      imageInputAvailable: true,
    });
    await settleLazyModules();
    screen.getByLabelText("Goal details");
    const options = screen.queryByRole("button", { name: "Composer options" });
    if (options) {
      fireEvent.click(options);
      await screen.findByRole("menu", { name: "Composer options menu" });
    }
    fireEvent.click(screen.getByRole("menuitem", { name: "Goal" }));
    expect(screen.queryByRole("menu")).toBeNull();
    expect(
      await screen.findByText("Ship safely", { selector: "p" }),
    ).toBeVisible();
    expect(screen.getByLabelText("Goal details")).toHaveFocus();
    expect(screen.getByRole("button", { name: "Pause goal" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Delete goal" })).toBeEnabled();
    for (const eligibility of [
      { availableBinding: false },
      { selectedHarnessId: "another-harness" },
    ]) {
      rerenderComposer(eligibility);
      const options = screen.queryByRole("button", {
        name: "Composer options",
      });
      if (options && !options.hasAttribute("disabled")) {
        fireEvent.click(options);
        await screen.findByRole("menu", { name: "Composer options menu" });
        expect(screen.queryByRole("menuitem", { name: "Goal" })).toBeNull();
        fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
      }
      expect(screen.queryByRole("menuitem", { name: "Goal" })).toBeNull();
      expect(screen.getByRole("button", { name: "Pause goal" })).toBeEnabled();
      expect(screen.getByRole("button", { name: "Delete goal" })).toBeEnabled();
    }
  });
  it("keeps an existing goal visible with an unavailable explanation while reloading", async () => {
    renderComposer({
      availableBinding: false,
      goal: {
        kind: "goal",
        snapshot: {
          objective: "Keep this goal",
          status: "active",
          tokenBudget: null,
          tokensUsed: 0,
          timeUsedSeconds: 0,
          createdAt: null,
          updatedAt: null,
        },
      },
    });
    await settleLazyModules();
    expect(screen.getByLabelText("Goal details")).toBeVisible();
    fireEvent.click(screen.getByLabelText("Goal details"));
    expect(
      await screen.findByText(/Native Goal actions are unavailable/),
    ).toBeVisible();
    expect(screen.queryByRole("button", { name: "Pause goal" })).toBeNull();
  });

  it("keeps errors beside the send action inside the joined dock while the input scrolls separately from the toolbar", async () => {
    const { rerenderComposer } = renderComposer({
      newChat: true,
      workbenchError: composerProblem(
        "workspace",
        "project-a",
        "Retry the request",
      ),
    });
    const form = screen.getByRole("form");
    const body = form.querySelector(".composer__body");
    expect(body).not.toBeNull();
    expect(body).toContainElement(
      screen.getByRole("combobox", { name: "Message agent" }),
    );
    const error = screen.getByRole("alert");
    const notices = screen.getByRole("region", { name: "Composer notices" });
    const location = await screen.findByRole("group", {
      name: "Chat location",
    });
    expect(error).toHaveTextContent("Retry the request");
    expect(notices).toContainElement(error);
    expect(notices).toHaveAttribute("tabindex", "0");
    notices.focus();
    expect(notices).toHaveFocus();
    expect(body).toContainElement(notices);
    expect(location.nextElementSibling).toContainElement(form);
    expect(form.closest(".chat-dock")).toContainElement(error);
    expect(body).not.toContainElement(form.querySelector(".composer__toolbar"));
    expect(form.querySelector(".composer__toolbar")?.parentElement).toBe(form);
    rerenderComposer({ hidden: true });
    expect(screen.queryByRole("group", { name: "Chat location" })).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(
      screen.queryByRole("region", { name: "Composer notices" }),
    ).toBeNull();
    rerenderComposer({ workbenchError: null });
    expect(
      screen.queryByRole("region", { name: "Composer notices" }),
    ).toBeNull();
  });

  it("keeps + open with nothing to attach refused in place, and refuses a dropped image", async () => {
    const { onAddImages } = renderComposer({
      imageInputDisabledReason: "This harness does not advertise image input.",
    });
    await openAttachMenu();
    expect(
      screen.getByRole("menuitem", { name: "Files and folders" }),
    ).toHaveAttribute("aria-disabled", "true");
    expect(screen.queryByRole("menu", { name: "Attach" })).toBeNull();
    fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
    expect(screen.queryByLabelText("Choose images")).toBeNull();
    fireEvent.drop(screen.getByRole("form"), {
      dataTransfer: { files: [png()], types: ["Files"] },
    });
    expect(onAddImages).not.toHaveBeenCalled();
    expect(
      screen
        .getByText("This harness does not advertise image input.")
        .closest(".notice"),
    ).toHaveClass("notice--info");
  });

  it("opens the attach menu for images where files are refused", async () => {
    renderComposer({ imageInputAvailable: true });
    await openAttachMenu();
    const attach = screen.getByRole("menu", { name: "Attach" });
    for (const pick of ["Files…", "Folder…"]) {
      const item = within(attach).getByRole("menuitem", { name: pick });
      expect(item).toBeDisabled();
      expect(item).toHaveAttribute(
        "title",
        "Attaching files needs the Sage desktop app with an ACP harness.",
      );
    }
    expect(
      within(attach).getByRole("menuitem", { name: "Images…" }),
    ).toBeEnabled();
  });

  it("accepts image paste and chooser only with capability", () => {
    const { onAddImages } = renderComposer({ imageInputAvailable: true });
    fireEvent.paste(screen.getByRole("combobox", { name: "Message agent" }), {
      clipboardData: { files: [png()] },
    });
    expect(onAddImages).toHaveBeenCalledWith([
      expect.objectContaining({ name: "sample.png" }),
    ]);
    const chooser = screen.getByLabelText("Choose images") as HTMLInputElement;
    fireEvent.change(chooser, { target: { files: [png()] } });
    expect(onAddImages).toHaveBeenCalledTimes(2);
  });

  it("refuses a pasted image with the session's reason when it lacks the capability", () => {
    const { onAddImages } = renderComposer({
      imageInputAvailable: false,
      imageInputDisabledReason: "This harness does not advertise image input.",
    });
    fireEvent.paste(screen.getByRole("combobox", { name: "Message agent" }), {
      clipboardData: { files: [png()] },
    });
    expect(onAddImages).not.toHaveBeenCalled();
    expect(
      screen
        .getByText("This harness does not advertise image input.")
        .closest(".notice"),
    ).toHaveClass("notice--info");
  });

  it("clears the unavailable warning once the session takes images", () => {
    const { rerenderComposer } = renderComposer({
      imageInputAvailable: false,
      imageInputDisabledReason:
        "Images become available once the session starts.",
    });
    fireEvent.paste(screen.getByRole("combobox", { name: "Message agent" }), {
      clipboardData: { files: [png()] },
    });
    expect(
      screen.getByText(/once the session starts/).closest(".notice"),
    ).toHaveClass("notice--info");
    rerenderComposer({
      imageInputAvailable: true,
      imageInputDisabledReason: null,
    });
    expect(screen.queryByText(/once the session starts/)).toBeNull();
  });

  it("passes large screenshots on for native resizing and refuses files over 10 MiB", () => {
    const { onAddImages } = renderComposer({ imageInputAvailable: true });
    const screenshot = new File([new Uint8Array(6 * 1024 * 1024)], "4k.png", {
      type: "image/png",
    });
    fireEvent.paste(screen.getByRole("combobox", { name: "Message agent" }), {
      clipboardData: { files: [screenshot, screenshot] },
    });
    expect(onAddImages).toHaveBeenCalledWith([screenshot, screenshot]);
    const huge = new File([new Uint8Array(10 * 1024 * 1024 + 1)], "huge.png", {
      type: "image/png",
    });
    fireEvent.paste(screen.getByRole("combobox", { name: "Message agent" }), {
      clipboardData: { files: [huge] },
    });
    expect(onAddImages).toHaveBeenCalledTimes(1);
    expect(
      screen
        .getByText("Choose PNG or JPEG images no larger than 10 MiB each.")
        .closest(".notice"),
    ).toHaveClass("notice--info");
  });

  it("opens the persistent chooser from Add's attach menu and closes it", async () => {
    renderComposer({ imageInputAvailable: true });
    const chooser = screen.getByLabelText("Choose images");
    const choose = vi.spyOn(chooser, "click");
    const trigger = screen.getByRole("button", { name: "Composer options" });
    await openAttachMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Images…" }));
    expect(choose).toHaveBeenCalledOnce();
    expect(chooser.isConnected).toBe(true);
    expect(screen.queryByRole("menu")).toBeNull();
    expect(trigger).toHaveFocus();
  });

  it("refuses a fifth image before upload", async () => {
    const { onAddImages } = renderComposer({
      imageInputAvailable: true,
      attachedContext: [0, 1, 2, 3].map((index) => ({
        kind: "image" as const,
        artifactId: `00000000-0000-4000-8000-00000000000${index}`,
        mediaType: "image/png" as const,
        width: 1,
        height: 1,
        byteLength: 1,
        sha256: "a".repeat(64),
        label: `image-${index}.png`,
      })),
    });
    fireEvent.drop(screen.getByRole("form"), {
      dataTransfer: { files: [png()], types: ["Files"] },
    });
    expect(onAddImages).not.toHaveBeenCalled();
    expect(screen.getByText(/at most 4 images/).closest(".notice")).toHaveClass(
      "notice--info",
    );
    await openAttachMenu();
    expect(screen.getByRole("menuitem", { name: "Images…" })).toBeDisabled();
  });

  it("shows an uploaded image thumbnail and removes its context", async () => {
    const onRemoveContext = vi.fn();
    const image = {
      kind: "image" as const,
      artifactId: "00000000-0000-4000-8000-000000000000",
      mediaType: "image/png" as const,
      width: 1,
      height: 1,
      byteLength: 1,
      sha256: "a".repeat(64),
      label: "sample.png",
    };
    renderComposer({
      imageInputAvailable: true,
      attachedContext: [image],
      imagePreviews: { [image.artifactId]: "blob:preview" },
      onRemoveContext,
    });
    await settleLazyModules();
    screen.getByLabelText("Attached context");
    expect(document.querySelector(".composer__image-preview")).toHaveAttribute(
      "src",
      "blob:preview",
    );
    expect(document.querySelector(".composer__body")).toContainElement(
      screen.getByRole("button", {
        name: /^Remove Image .*sample\.png context$/u,
      }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Remove Image · sample.png context" }),
    );
    expect(onRemoveContext).toHaveBeenCalledWith(image);
  });

  it("enables Send with only an attachment and no text (Codex s2/f0095)", async () => {
    const image = {
      kind: "image" as const,
      artifactId: "00000000-0000-4000-8000-000000000000",
      mediaType: "image/png" as const,
      width: 1,
      height: 1,
      byteLength: 1,
      sha256: "a".repeat(64),
      label: "sample.png",
    };
    const { rerenderComposer } = renderComposer({
      text: "",
      imageInputAvailable: true,
      attachedContext: [],
    });
    expect(
      screen.getByRole("button", { name: /dictation|Voice input/ }),
    ).toBeVisible();
    rerenderComposer({
      text: "  ",
      imageInputAvailable: true,
      attachedContext: [image],
    });
    await waitFor(
      () =>
        expect(
          screen.getByRole("button", { name: "Send message" }),
        ).toBeEnabled(),
      { timeout: 5_000 },
    );
  });

  it("keeps attachment-only Send paired with a visible page chip, then restores the mic on removal", async () => {
    const page = {
      kind: "browserPage" as const,
      tabId: "browser-1",
      url: "https://example.com",
      title: "Example page",
      text: "Page text",
    };
    const { rerenderComposer } = renderComposer({
      text: "  ",
      attachedContext: [page],
    });
    await waitFor(
      () =>
        expect(
          screen.getByRole("button", { name: "Send message" }),
        ).toBeEnabled(),
      { timeout: 5_000 },
    );
    expect(await screen.findByText("Page · Example page")).toBeVisible();
    rerenderComposer({ text: "  ", attachedContext: [] });
    expect(
      screen.getByRole("button", { name: /dictation|Voice input/ }),
    ).toBeVisible();
    expect(screen.queryByRole("button", { name: "Send message" })).toBeNull();
  });

  it("does not submit with an unsupported image or while upload is pending", () => {
    const onSubmit = vi.fn((event) => event.preventDefault());
    const image = {
      kind: "image" as const,
      artifactId: "00000000-0000-4000-8000-000000000000",
      mediaType: "image/png" as const,
      width: 1,
      height: 1,
      byteLength: 1,
      sha256: "a".repeat(64),
      label: "sample.png",
    };
    const { unmount } = renderComposer({ attachedContext: [image], onSubmit });
    const form = screen.getByRole("form") as HTMLFormElement;
    form.requestSubmit = vi.fn(() => fireEvent.submit(form));
    fireEvent.keyDown(screen.getByRole("combobox", { name: "Message agent" }), {
      key: "Enter",
    });
    expect(form.requestSubmit).toHaveBeenCalled();
    fireEvent.submit(screen.getByRole("form"));
    expect(onSubmit).not.toHaveBeenCalled();
    unmount();
    renderComposer({
      imageInputAvailable: true,
      imageUploading: true,
      onSubmit,
    });
    fireEvent.submit(screen.getByRole("form"));
    expect(onSubmit).not.toHaveBeenCalled();
  });
});
