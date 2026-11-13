import {
  AlertTriangle,
  FilePen,
  ListChecks,
  Sparkles,
} from "../../ui/leanIcons";
import { ACCESS_ICONS } from "./composerAccessIcons";

/** Every access mode's icon. Loaded with a chat's access, off the chat path. */
export const CHAT_ACCESS_ICONS = {
  ...ACCESS_ICONS,
  acceptEdits: FilePen,
  plan: ListChecks,
  auto: Sparkles,
  fullAccess: AlertTriangle,
};
