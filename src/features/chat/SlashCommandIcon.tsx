import { createElement } from "react";

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
  // The icon is a module-level component chosen by name, so it never
  // remounts; createElement says so where JSX on a local would not.
  return createElement(commandIcons.get(name) ?? Command, {
    size: 14,
    "aria-hidden": "true",
  });
}
