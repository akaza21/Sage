import {
  Command,
  FileDiff,
  Info,
  ListChecks,
  ListFilter,
} from "../../ui/leanIcons";

const iconProps = { size: 14, "aria-hidden": true } as const;

export function SlashCommandIcon({ name }: { name: string }) {
  switch (name) {
    case "compact":
      return <ListFilter {...iconProps} />;
    case "review":
      return <FileDiff {...iconProps} />;
    case "status":
      return <Info {...iconProps} />;
    case "plan":
      return <ListChecks {...iconProps} />;
    default:
      return <Command {...iconProps} />;
  }
}
