import {
  MAX_IMAGE_LABEL_BYTES,
  MAX_IMAGE_OWNER_ID_BYTES,
  MAX_IMAGE_UPLOAD_BYTES,
} from "../domain/attachedContext";

// JSON can escape each byte as six ASCII bytes. Reserve 20 decimal digits for
// each of the two counters.
const MAX_UPLOAD_METADATA_BYTES =
  (2 * MAX_IMAGE_OWNER_ID_BYTES + MAX_IMAGE_LABEL_BYTES) * 6 + 2 * 20;
const UPLOAD_ENVELOPE =
  '{"id":,"cmd":"upload_chat_image","args":{"request":{"projectId":"","projectGeneration":,"chatId":"","mediaType":"image/jpeg","dataBase64":"","label":""}}}';

export const MAX_MESSAGE_BYTES =
  Math.ceil(MAX_IMAGE_UPLOAD_BYTES / 3) * 4 +
  MAX_UPLOAD_METADATA_BYTES +
  UPLOAD_ENVELOPE.length;
