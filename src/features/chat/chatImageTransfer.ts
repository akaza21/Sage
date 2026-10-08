import { useWorkbenchStore } from "../../app/store";
import {
  MAX_OUTGOING_IMAGE_BYTES,
  type ImageAttachedContext,
} from "../../domain/attachedContext";
import type { ChatOwnershipIntent } from "../../domain/chatOwnership";
import { invokeNative } from "../../ports/nativeInvoke";

function readImageFile(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(new Error("Could not read image."));
    reader.onload = () => {
      const value = reader.result;
      if (typeof value !== "string") {
        reject(new Error("Could not read image."));
        return;
      }
      const separator = value.indexOf(",");
      if (separator < 0) {
        reject(new Error("Could not read image."));
        return;
      }
      resolve(value.slice(separator + 1));
    };
    reader.readAsDataURL(file);
  });
}

export async function removeChatImage(
  intent: ChatOwnershipIntent,
  artifactId: string,
) {
  await invokeNative("remove_chat_image", {
    request: {
      projectId: intent.projectId,
      projectGeneration: intent.transitionEpoch,
      chatId: intent.chatId,
      artifactId,
    },
  });
}

export async function uploadChatImages(
  intent: ChatOwnershipIntent,
  files: File[],
  onPreview: (artifactId: string, file: File) => void,
) {
  for (const file of files) {
    if (
      !["image/png", "image/jpeg"].includes(file.type) ||
      file.size < 1 ||
      file.size > MAX_OUTGOING_IMAGE_BYTES
    )
      throw new Error("Choose PNG or JPEG images no larger than 4 MiB each.");
    const image = await invokeNative<ImageAttachedContext>(
      "upload_chat_image",
      {
        request: {
          projectId: intent.projectId,
          projectGeneration: intent.transitionEpoch,
          chatId: intent.chatId,
          mediaType: file.type,
          dataBase64: await readImageFile(file),
          label: file.name.slice(0, 80),
        },
      },
    );
    if (!useWorkbenchStore.getState().attachChatContext(intent, image)) {
      await removeChatImage(intent, image.artifactId);
      throw new Error("Chat changed before the image could be attached.");
    }
    onPreview(image.artifactId, file);
  }
}
