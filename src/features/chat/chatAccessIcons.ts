import {
  AlertTriangle,
  FilePen,
  ListChecks,
  Sparkles,
} from "../../ui/leanIcons";
import { ACCESS_ICONS } from "./composerAccessIcons";

export const CHAT_ACCESS_ICONS = {
  ...ACCESS_ICONS,
  acceptEdits: FilePen,
  plan: ListChecks,
  auto: Sparkles,
  fullAccess: AlertTriangle,
};
