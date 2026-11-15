import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useWorkbenchStore } from "../../app/store";
import type { ImageAttachedContext } from "../../domain/attachedContext";
import { useChatImageAttachments } from "./useChatImageAttachments";

const transfer = vi.hoisted(() => ({
  upload: vi.fn(),
  remove: vi.fn(),
  read: vi.fn(),
}));
vi.mock("./chatImageTransfer", () => ({
  uploadChatImages: transfer.upload,
  removeChatImage: transfer.remove,
  readChatImageThumbnail: transfer.read,
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
  for (const mock of Object.values(transfer)) mock.mockReset();
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

function stubObjectUrls() {
  let created = 0;
  const urls = {
    create: vi.fn(() => `blob:thumbnail-${++created}`),
    revoke: vi.fn(),
  };
  vi.stubGlobal("URL", {
    createObjectURL: urls.create,
    revokeObjectURL: urls.revoke,
  });
  return urls;
}

function renderAttachments() {
  return renderHook(
    ({ contexts }) => useChatImageAttachments("chat-a", contexts),
    { initialProps: { contexts: [image] as ImageAttachedContext[] } },
  );
}

it("previews an attached image's native thumbnail and releases it once the image is gone", async () => {
  const urls = stubObjectUrls();
  seedChat();
  transfer.read.mockResolvedValue(new Blob(["png"]));
  const view = renderAttachments();

  await waitFor(() =>
    expect(view.result.current.imagePreviews).toEqual({
      [image.artifactId]: "blob:thumbnail-1",
    }),
  );
  expect(transfer.read).toHaveBeenCalledWith(
    expect.objectContaining({ projectId: "project-a", chatId: "chat-a" }),
    image.artifactId,
  );

  view.rerender({ contexts: [] });
  expect(urls.revoke).toHaveBeenCalledWith("blob:thumbnail-1");
});

it("creates no object URL for a thumbnail that arrives after its image was removed", async () => {
  const urls = stubObjectUrls();
  seedChat();
  let deliver!: (thumbnail: Blob) => void;
  transfer.read.mockReturnValue(
    new Promise<Blob>((resolve) => {
      deliver = resolve;
    }),
  );
  const view = renderAttachments();
  await waitFor(() => expect(transfer.read).toHaveBeenCalled());

  view.rerender({ contexts: [] });
  await act(async () => deliver(new Blob(["png"])));

  expect(urls.create).not.toHaveBeenCalled();
  expect(view.result.current.imagePreviews).toEqual({});
});

function attachInStore() {
  useWorkbenchStore.setState((state) => ({
    chatUiById: {
      "chat-a": { ...state.chatUiById["chat-a"]!, attachedContext: [image] },
    },
  }));
}

it("keeps showing the same thumbnail when a failed removal restores the image", async () => {
  const urls = stubObjectUrls();
  seedChat();
  attachInStore();
  transfer.read.mockResolvedValue(new Blob(["png"]));
  transfer.remove.mockRejectedValue(new Error("disk busy"));
  const view = renderAttachments();
  await waitFor(() =>
    expect(view.result.current.imagePreviews[image.artifactId]).toBeDefined(),
  );

  act(() => view.result.current.removeAttachedContext(image));
  view.rerender({ contexts: [] });
  await waitFor(() =>
    expect(
      useWorkbenchStore.getState().chatUiById["chat-a"]!.attachedContext,
    ).toEqual([image]),
  );
  view.rerender({ contexts: [image] });

  expect(view.result.current.imagePreviews).toEqual({
    [image.artifactId]: "blob:thumbnail-1",
  });
  expect(urls.revoke).not.toHaveBeenCalled();
  expect(transfer.read).toHaveBeenCalledTimes(1);
});

it("releases the thumbnail when a failed removal cannot restore the image because a send started", async () => {
  const urls = stubObjectUrls();
  seedChat();
  attachInStore();
  transfer.read.mockResolvedValue(new Blob(["png"]));
  let refuse!: (error: Error) => void;
  transfer.remove.mockReturnValue(
    new Promise<void>((_, reject) => {
      refuse = reject;
    }),
  );
  const view = renderAttachments();
  await waitFor(() =>
    expect(view.result.current.imagePreviews[image.artifactId]).toBeDefined(),
  );

  act(() => view.result.current.removeAttachedContext(image));
  view.rerender({ contexts: [] });
  await waitFor(() => expect(transfer.remove).toHaveBeenCalled());
  useWorkbenchStore.setState((state) => ({
    chatUiById: {
      "chat-a": { ...state.chatUiById["chat-a"]!, sending: true },
    },
  }));
  await act(async () => refuse(new Error("disk busy")));

  expect(
    useWorkbenchStore.getState().chatUiById["chat-a"]!.attachedContext,
  ).toEqual([]);
  expect(urls.revoke).toHaveBeenCalledWith("blob:thumbnail-1");
  expect(view.result.current.imagePreviews).toEqual({});
});

it("releases a removed image's thumbnail once native code retires it", async () => {
  const urls = stubObjectUrls();
  seedChat();
  attachInStore();
  transfer.read.mockResolvedValue(new Blob(["png"]));
  transfer.remove.mockResolvedValue(undefined);
  const view = renderAttachments();
  await waitFor(() =>
    expect(view.result.current.imagePreviews[image.artifactId]).toBeDefined(),
  );

  act(() => view.result.current.removeAttachedContext(image));
  view.rerender({ contexts: [] });

  await waitFor(() => expect(view.result.current.imagePreviews).toEqual({}));
  expect(urls.revoke).toHaveBeenCalledWith("blob:thumbnail-1");
});

it("releases every thumbnail when the composer unmounts", async () => {
  const urls = stubObjectUrls();
  seedChat();
  transfer.read.mockResolvedValue(new Blob(["png"]));
  const view = renderAttachments();
  await waitFor(() =>
    expect(view.result.current.imagePreviews[image.artifactId]).toBeDefined(),
  );

  view.unmount();

  expect(urls.revoke).toHaveBeenCalledWith("blob:thumbnail-1");
});

it("stops showing a released thumbnail even if the restored image's thumbnail cannot be read", async () => {
  stubObjectUrls();
  seedChat();
  transfer.read
    .mockResolvedValueOnce(new Blob(["png"]))
    .mockResolvedValueOnce(null);
  const view = renderAttachments();
  await waitFor(() =>
    expect(view.result.current.imagePreviews[image.artifactId]).toBeDefined(),
  );

  view.rerender({ contexts: [] });
  view.rerender({ contexts: [image] });
  await waitFor(() => expect(transfer.read).toHaveBeenCalledTimes(2));

  expect(view.result.current.imagePreviews).toEqual({});
});
