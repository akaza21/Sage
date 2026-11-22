import type { ReactNode } from "react";
import { commandSources } from "../../domain/acpCommand";
import type {
  AcpCommandSection,
  AcpNativeCommandControl,
} from "../../domain/acpCommandPlacement";
import type { AvailableAcpCommand } from "../../services/acpCommandsDesktop";

export const sectionLabels = {
  addToMessage: "Add to message",
  session: "Session",
  connections: "Connections",
  addOns: "Add-ons",
  skillsAndCommands: "Skills and commands",
  modelAndAccess: "Model and access",
  usage: "Usage",
  more: "More",
} satisfies Record<AcpCommandSection, string>;

const sourceLabels = {
  builtin: "Commands",
  skill: "Skills",
  prompt: "Prompts",
  extension: "Extensions",
  mcp: "MCP prompts",
  plugin: "Plugins",
  harness: "Other commands",
};

export interface ComposerNativeRow {
  kind: "native";
  id: string;
  section: AcpCommandSection;
  label: string;
  description?: string;
  control?: AcpNativeCommandControl;
  content: ReactNode;
}

export type ComposerMenuRow =
  | ComposerNativeRow
  | { kind: "command"; id: string; command: AvailableAcpCommand };

export interface ComposerMenuSection {
  id: AcpCommandSection;
  label: string;
  groups: { label?: string; rows: ComposerMenuRow[] }[];
  more: number;
}

/** Inputs are bounded by the command/config decoders. Only 50 matching rows
 * per section reach React; searching still considers the complete catalog. */
export function composerMenuSections(
  native: readonly ComposerNativeRow[],
  catalog: readonly AvailableAcpCommand[],
  query: string,
): ComposerMenuSection[] {
  const needle = query.trim().toLocaleLowerCase();
  const controls = new Set(native.map((row) => row.control).filter(Boolean));
  const matches = (...text: (string | undefined | null)[]) =>
    text.some((value) => value?.toLocaleLowerCase().includes(needle));
  const matchesCommand = (
    command: AvailableAcpCommand,
    id: AcpCommandSection,
  ) =>
    matches(
      command.name,
      command.description,
      command.inputHint,
      command.sourceLabel,
      sourceLabels[command.source],
      sectionLabels[id],
      ...(command.aliases ?? []),
    );
  return (Object.keys(sectionLabels) as AcpCommandSection[]).flatMap((id) => {
    const rows: ComposerMenuRow[] = native.filter(
      (row) =>
        row.section === id &&
        (matches(row.label, row.description, sectionLabels[id]) ||
          (row.control !== undefined &&
            catalog.some(
              (command) =>
                command.placement?.control === row.control &&
                matchesCommand(command, id),
            ))),
    );
    const commands = catalog.filter(
      (command) =>
        command.placement?.section === id &&
        // More is the complete escape hatch, even for native controls.
        (id === "more" || !controls.has(command.placement.control)) &&
        matchesCommand(command, id),
    );
    const groups: ComposerMenuSection["groups"] = [];
    if (id === "more") {
      for (const source of commandSources) {
        const labels = new Map<string, AvailableAcpCommand[]>();
        for (const command of commands) {
          if (command.source !== source) continue;
          const label = command.sourceLabel ?? "";
          const group = labels.get(label) ?? [];
          group.push(command);
          labels.set(label, group);
        }
        for (const [label, entries] of labels) {
          groups.push({
            label: [sourceLabels[source], label].filter(Boolean).join(" · "),
            rows: entries.map((command) => ({
              kind: "command",
              id: command.name,
              command,
            })),
          });
        }
      }
      if (rows.length) groups.push({ rows });
    } else {
      groups.push({
        rows: [
          ...rows,
          ...commands.map((command) => ({
            kind: "command" as const,
            id: command.name,
            command,
          })),
        ],
      });
    }
    const count = groups.reduce((sum, group) => sum + group.rows.length, 0);
    let remaining = 50;
    const visible = groups.flatMap((group) => {
      const shown = group.rows.slice(0, remaining);
      remaining -= shown.length;
      return shown.length ? [{ ...group, rows: shown }] : [];
    });
    return count
      ? [
          {
            id,
            label: sectionLabels[id],
            groups: visible,
            more: Math.max(0, count - 50),
          },
        ]
      : [];
  });
}
