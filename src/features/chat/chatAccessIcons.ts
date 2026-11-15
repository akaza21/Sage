import { CircleAlert, Pencil, ShieldCheck } from "../../ui/leanIcons";
import { Lightbulb } from "../../ui/leanIconNodesLazy";
import type { SessionAccessMode } from "../../domain/sessionAccessMode";
import { ACCESS_ICONS } from "./composerAccessIcons";

export const CHAT_ACCESS_ICONS = {
  ...ACCESS_ICONS,
  acceptEdits: Pencil,
  plan: Lightbulb,
  auto: ShieldCheck,
  fullAccess: CircleAlert,
} satisfies Record<SessionAccessMode, typeof Pencil>;
