import { invokeNative } from "../ports/nativeInvoke";

export interface GetChatImageThumbnailRequest {
  projectId: string;
  projectGeneration: string;
  chatId: string;
  artifactId: string;
}

export async function getChatImageThumbnail(
  request: GetChatImageThumbnailRequest,
): Promise<string | null> {
  const response: unknown = await invokeNative("get_chat_image_thumbnail", {
    request,
  });
  if (response === null) return null;
  if (
    !response ||
    typeof response !== "object" ||
    Array.isArray(response) ||
    !("dataUrl" in response) ||
    typeof response.dataUrl !== "string" ||
    response.dataUrl.length > 350_000 ||
    !/^data:image\/(png|jpeg);base64,[A-Za-z0-9+/]+={0,2}$/.test(
      response.dataUrl,
    )
  ) {
    throw new Error("Image thumbnail response is invalid");
  }
  return response.dataUrl;
}

export async function discardPendingChatImage(
  request: GetChatImageThumbnailRequest,
): Promise<boolean> {
  const response: unknown = await invokeNative("discard_pending_chat_image", {
    request,
  });
  if (typeof response !== "boolean") {
    throw new Error("Image discard response is invalid");
  }
  return response;
}
