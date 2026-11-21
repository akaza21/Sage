import { isRecord } from "./guards";

export const commandSections = [
  "addToMessage",
  "session",
  "connections",
  "addOns",
  "skillsAndCommands",
  "modelAndAccess",
  "usage",
  "more",
] as const;
export type AcpCommandSection = (typeof commandSections)[number];

export const nativeCommandControls = [
  "goalEditor",
  "compact",
  "modelPicker",
  "effortPicker",
  "mcpPanel",
  "usagePanel",
  "review",
  "newChat",
  "accessMenu",
  "reloadAddOns",
] as const;
export type AcpNativeCommandControl = (typeof nativeCommandControls)[number];

export interface AcpCommandPlacement {
  section: AcpCommandSection;
  control?: AcpNativeCommandControl;
}

export function isCommandPlacement(
  value: unknown,
): value is AcpCommandPlacement {
  return (
    isRecord(value) &&
    Object.keys(value).length === (value.control === undefined ? 1 : 2) &&
    commandSections.includes(value.section as AcpCommandSection) &&
    (value.control === undefined ||
      nativeCommandControls.includes(value.control as AcpNativeCommandControl))
  );
}
