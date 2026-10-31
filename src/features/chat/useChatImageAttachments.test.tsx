import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useWorkbenchStore } from "../../app/store";
import type { ImageAttachedContext } from "../../domain/attachedContext";
import { useChatImageAttachments } from "./useChatImageAttachments";

const transfer = vi.hoisted(() => ({ upload: vi.fn(), remove: vi.fn() }));
vi.mock("./chatImageTransfer", () => ({
  uploadChatImages: transfer.upload,
  removeChatImage: transfer.remove,
}));
vi.mock("../../ports/nativeInvoke", () => ({
  isTauriRuntime: () => true,
  invokeNative: vi.fn(async () => []),
}));

const initialState = useWorkbenchStore.getState();
afterEach(() => {
  useWorkbenchStore.setState(initialState, true);
  vi.unstubAllGlobals();
  vi.clearAllMocks();
});

it("keeps an image thumbnail available when native removal fails and its chip returns", async () => {
  const image: ImageAttachedContext = {
    kind: "image",
    artifactId: "00000000-0000-4000-8000-000000000000",
    mediaType: "image/png",
    width: 1,
    height: 1,
    byteLength: 1,
    sha256: "a".repeat(64),
    label: "sample.png",
  };
  const revoke = vi.fn();
  vi.stubGlobal(
    "URL",
    class extends URL {
      static createObjectURL = () => "blob:preview";
      static revokeObjectURL = revoke;
    },
  );
  useWorkbenchStore.setState({
    project: { id: "project-a", name: "A", roots: [] },
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
    activeTabId: "chat-a",
    chatOwnershipEpoch: 1,
    chatUiById: {
      "chat-a": {
        composerText: "",
        attachedContext: [],
        sending: false,
        error: null,
        errorKind: null,
        requestGeneration: 0,
      },
    },
  });
  transfer.upload.mockImplementation(async (intent, files, onPreview) => {
    useWorkbenchStore.getState().attachChatContext(intent, image);
    onPreview(image.artifactId, files[0]);
  });
  let rejectRemoval!: (error: Error) => void;
  transfer.remove.mockImplementation(
    () =>
      new Promise<void>((_, reject) => {
        rejectRemoval = reject;
      }),
  );
  let view = renderHook(
    ({ contexts }) => useChatImageAttachments("chat-a", contexts),
    {
      initialProps: { contexts: [] as ImageAttachedContext[] },
    },
  );
  await act(async () =>
    view.result.current.addImages([
      new File(["x"], "sample.png", { type: "image/png" }),
    ]),
  );
  view.rerender({ contexts: [image] });
  expect(useWorkbenchStore.getState().chatUiById["chat-a"].error).toBeNull();
  expect(view.result.current.imagePreviews[image.artifactId]).toBe(
    "blob:preview",
  );

  view.unmount();
  view = renderHook(
    ({ contexts }) => useChatImageAttachments("chat-a", contexts),
    { initialProps: { contexts: [image] } },
  );
  expect(view.result.current.imagePreviews[image.artifactId]).toBe(
    "blob:preview",
  );
  expect(
    useWorkbenchStore.getState().chatUiById["chat-a"].attachedContext,
  ).toEqual([image]);

  act(() => view.result.current.removeAttachedContext(image));
  view.rerender({ contexts: [] });
  await waitFor(() => expect(transfer.remove).toHaveBeenCalled());
  await act(async () => {
    rejectRemoval(new Error("disk busy"));
  });
  view.rerender({ contexts: [image] });
  await waitFor(() =>
    expect(view.result.current.imagePreviews[image.artifactId]).toBe(
      "blob:preview",
    ),
  );
  expect(revoke).not.toHaveBeenCalledWith("blob:preview");
  useWorkbenchStore.setState({
    project: { id: "project-b", name: "B", roots: [] },
  });
  view.rerender({ contexts: [image] });
  expect(view.result.current.imagePreviews[image.artifactId]).toBeUndefined();
});
