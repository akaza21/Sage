import { afterEach, expect, it, vi } from "vitest";
import { waitFor } from "@testing-library/react";
import { useWorkbenchStore } from "../../app/store";
import type { ImageAttachedContext } from "../../domain/attachedContext";
import { removeDraftImage } from "./chatImagePreviewCache";

const native = vi.hoisted(() => ({ pending: [] as (() => void)[] }));
vi.mock("../../ports/nativeInvoke", () => ({
  isTauriRuntime: () => false,
  invokeNative: (command: string) => {
    if (command !== "remove_chat_image")
      throw new Error(`Unexpected command ${command}`);
    return new Promise<void>((resolve) => native.pending.push(resolve));
  },
}));
const initial = useWorkbenchStore.getState();
afterEach(() => {
  native.pending.splice(0).forEach((resolve) => resolve());
  useWorkbenchStore.setState(initial, true);
});

it("refuses excess pending image removals before changing the ninth draft", async () => {
  const images: ImageAttachedContext[] = Array.from(
    { length: 9 },
    (_, index) => ({
      kind: "image",
      artifactId: `00000000-0000-4000-8000-${String(index + 1).padStart(12, "0")}`,
      mediaType: "image/png",
      width: 1,
      height: 1,
      byteLength: 1,
      sha256: "a".repeat(64),
      label: `image-${index}.png`,
    }),
  );
  useWorkbenchStore.setState({
    project: { id: "project-a", name: "A", roots: [] },
    projectTransitioning: false,
    tabs: images.map((_, index) => ({
      kind: "chat",
      id: `chat-${index}`,
      title: "Chat",
      position: index,
      pinned: false,
      dirty: false,
    })),
    chatUiById: Object.fromEntries(
      images.map((image, index) => [
        `chat-${index}`,
        {
          composerText: "",
          attachedContext: [image],
          sending: false,
          error: null,
          errorKind: null,
          requestGeneration: 0,
        },
      ]),
    ),
  });
  const operations = images.map((image, index) => {
    const intent = useWorkbenchStore
      .getState()
      .captureChatOwnership(`chat-${index}`);
    if (!intent) throw new Error("Expected owned chat");
    return removeDraftImage(intent, image);
  });
  try {
    expect(
      useWorkbenchStore.getState().chatUiById["chat-8"].attachedContext,
    ).toEqual([images[8]]);
    expect(useWorkbenchStore.getState().chatUiById["chat-8"].error).toContain(
      "Wait for pending image operations",
    );
    expect(
      useWorkbenchStore.getState().chatUiById["chat-0"].attachedContext,
    ).toEqual([]);
    await waitFor(() => expect(native.pending).toHaveLength(8));
  } finally {
    native.pending.splice(0).forEach((resolve) => resolve());
    await Promise.all(operations);
  }
});
