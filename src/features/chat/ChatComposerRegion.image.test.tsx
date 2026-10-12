import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ComponentProps } from "react";

import { ChatComposerRegion } from "./ChatComposerRegion";

const png = () => new File(["png"], "sample.png", { type: "image/png" });
afterEach(cleanup);

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
    permissionRequests: false,
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
  return { onAddImages, ...view };
}

describe("composer image input", () => {
  it("keeps upper composer content in a scroll region separate from the toolbar", () => {
    renderComposer({ workbenchError: "Retry the request" });
    const form = screen.getByRole("form");
    const body = form.querySelector(".composer__body");
    expect(body).not.toBeNull();
    expect(body).toContainElement(screen.getByRole("textbox"));
    expect(body).toContainElement(screen.getByRole("alert"));
    expect(body).not.toContainElement(form.querySelector(".composer__toolbar"));
    expect(form.querySelector(".composer__toolbar")?.parentElement).toBe(form);
  });

  it("explains unavailable capability and refuses a dropped image", () => {
    const { onAddImages } = renderComposer();
    expect(screen.getByRole("button", { name: "Add images" })).toBeDisabled();
    const toolbar = document.querySelector(".composer__toolbar");
    const options = screen.getByRole("button", { name: "Composer options" });
    const image = screen.getByRole("button", { name: "Add images" });
    const harness = screen.getByRole("button", { name: "Harness" });
    expect(toolbar?.querySelector(".composer__controls")?.contains(image)).toBe(
      true,
    );
    expect(
      options.compareDocumentPosition(image) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(
      image.compareDocumentPosition(harness) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    fireEvent.drop(screen.getByRole("form"), {
      dataTransfer: { files: [png()], types: ["Files"] },
    });
    expect(onAddImages).not.toHaveBeenCalled();
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Image input is unavailable",
    );
  });

  it("accepts image paste and chooser only with capability", () => {
    const { onAddImages } = renderComposer({ imageInputAvailable: true });
    fireEvent.paste(screen.getByRole("textbox", { name: "Message agent" }), {
      clipboardData: { files: [png()] },
    });
    expect(onAddImages).toHaveBeenCalledWith([
      expect.objectContaining({ name: "sample.png" }),
    ]);
    const chooser = screen.getByLabelText("Choose images") as HTMLInputElement;
    fireEvent.change(chooser, { target: { files: [png()] } });
    expect(onAddImages).toHaveBeenCalledTimes(2);
  });

  it("refuses a fifth image before upload", () => {
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
    expect(screen.getByRole("alert")).toHaveTextContent("four images");
  });

  it("shows an uploaded image thumbnail and removes its context", () => {
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
    fireEvent.keyDown(screen.getByRole("textbox", { name: "Message agent" }), {
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
