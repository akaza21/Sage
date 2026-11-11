import {
  Command,
  FileDiff,
  Info,
  ListChecks,
  ListFilter,
} from "../../ui/leanIcons";

const commandIcons = new Map([
  ["compact", ListFilter],
  ["review", FileDiff],
  ["status", Info],
  ["plan", ListChecks],
]);

export function SlashCommandIcon({ name }: { name: string }) {
  const Icon = commandIcons.get(name) ?? Command;
  return <Icon size={14} aria-hidden="true" />;
}
