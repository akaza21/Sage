import type * as SlashChunk from "./ComposerSlashController";
import { lazyParts } from "./lazyParts";

/** The `/` controller facade: composer commands and a chat's first send. */
export const SlashPart = lazyParts<
  Pick<typeof SlashChunk, "ComposerSlashController" | "FirstChatSend">
>(() => import("./ComposerSlashController"));

/**
 * The session strip's chunk. The composer mounts its chrome gate from it,
 * which the transcript already loads for any chat with a session surface;
 * the gate fetches the chrome itself only while the session reports some.
 */
export { NoticePart } from "./lazyParts";
