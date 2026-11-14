import {
  CircleAlert,
  Lightbulb,
  Pencil,
  ShieldCheck,
} from "../../ui/leanIcons";
import type { SessionAccessMode } from "../../domain/sessionAccessMode";
import { ACCESS_ICONS } from "./composerAccessIcons";

/** Every access mode's icon. Loaded with a chat's access, off the chat path. */
export const CHAT_ACCESS_ICONS = {
  ...ACCESS_ICONS,
  acceptEdits: Pencil,
  plan: Lightbulb,
  auto: ShieldCheck,
  fullAccess: CircleAlert,
} satisfies Record<SessionAccessMode, typeof Pencil>;
