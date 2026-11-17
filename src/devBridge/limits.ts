import { MAX_IMAGE_UPLOAD_BYTES } from "../domain/attachedContext";

// Upload owners allow 512 bytes each and labels 80; JSON can escape each byte
// as six ASCII bytes. Reserve 20 decimal digits for each of the two counters.
const MAX_UPLOAD_METADATA_BYTES = (2 * 512 + 80) * 6 + 2 * 20;
const UPLOAD_ENVELOPE =
  '{"id":,"cmd":"upload_chat_image","args":{"request":{"projectId":"","projectGeneration":,"chatId":"","mediaType":"image/jpeg","dataBase64":"","label":""}}}';

export const MAX_MESSAGE_BYTES =
  Math.ceil(MAX_IMAGE_UPLOAD_BYTES / 3) * 4 +
  MAX_UPLOAD_METADATA_BYTES +
  UPLOAD_ENVELOPE.length;
