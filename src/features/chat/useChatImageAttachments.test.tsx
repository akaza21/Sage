import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useWorkbenchStore } from "../../app/store";
import {
  attachedContextKey,
  type ImageAttachedContext,
} from "../../domain/attachedContext";
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

function seedChat() {
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
}

it("keeps an image thumbnail available when native removal fails and its chip returns", async () => {
  const revoke = vi.fn();
  vi.stubGlobal("URL", {
    createObjectURL: () => "blob:preview",
    revokeObjectURL: revoke,
  });
  seedChat();
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
  const view = renderHook(
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
  expect(view.result.current.imagePreviews[image.artifactId]).toBe(
    "blob:preview",
  );

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
});

it("creates no object URL for a thumbnail that finishes after its chip was removed", async () => {
  const create = vi.fn(() => "blob:preview");
  vi.stubGlobal("URL", { createObjectURL: create, revokeObjectURL: vi.fn() });
  seedChat();
  transfer.upload.mockImplementation(async (intent, files, onPreview) => {
    const state = useWorkbenchStore.getState();
    state.attachChatContext(intent, image);
    state.detachChatContext(intent, attachedContextKey(image));
    onPreview(image.artifactId, files[0]);
  });
  const view = renderHook(() => useChatImageAttachments("chat-a", []));
  await act(async () =>
    view.result.current.addImages([
      new File(["x"], "sample.png", { type: "image/png" }),
    ]),
  );
  expect(create).not.toHaveBeenCalled();
  expect(view.result.current.imagePreviews).toEqual({});
});

it("keeps a thumbnail that finishes during a removal that then rolls back", async () => {
  const revoke = vi.fn();
  vi.stubGlobal("URL", {
    createObjectURL: () => "blob:preview",
    revokeObjectURL: revoke,
  });
  seedChat();
  let finishThumbnail!: () => void;
  transfer.upload.mockImplementation(async (intent, files, onPreview) => {
    useWorkbenchStore.getState().attachChatContext(intent, image);
    await new Promise<void>((resolve) => {
      finishThumbnail = resolve;
    });
    onPreview(image.artifactId, files[0]);
  });
  let rejectRemoval!: (error: Error) => void;
  transfer.remove.mockImplementation(
    () =>
      new Promise<void>((_, reject) => {
        rejectRemoval = reject;
      }),
  );
  const view = renderHook(
    ({ contexts }) => useChatImageAttachments("chat-a", contexts),
    { initialProps: { contexts: [] as ImageAttachedContext[] } },
  );
  let upload!: Promise<void>;
  act(() => {
    upload = view.result.current.addImages([
      new File(["x"], "sample.png", { type: "image/png" }),
    ]);
  });
  await waitFor(() => expect(finishThumbnail).toBeDefined());
  view.rerender({ contexts: [image] });
  act(() => view.result.current.removeAttachedContext(image));
  view.rerender({ contexts: [] });
  await waitFor(() => expect(transfer.remove).toHaveBeenCalled());

  await act(async () => {
    finishThumbnail();
    await upload;
  });
  await act(async () => {
    rejectRemoval(new Error("disk busy"));
  });
  view.rerender({ contexts: [image] });

  await waitFor(() =>
    expect(view.result.current.imagePreviews[image.artifactId]).toBe(
      "blob:preview",
    ),
  );
  expect(revoke).not.toHaveBeenCalled();
});
