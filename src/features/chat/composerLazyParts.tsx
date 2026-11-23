import type * as SlashChunk from "./ComposerSlashController";
import { lazyParts } from "./lazyParts";

/**
 * The composer's small deferred facade: slash commands, and harness chrome,
 * which renders nothing until a session reports some.
 */
export const SlashPart = lazyParts<
  Pick<typeof SlashChunk, "ComposerSlashController" | "ComposerChrome">
>(() => import("./ComposerSlashController"));
