import type { ToolPresentation } from "../../../domain/acp";
import {
  ArrowRightLeft,
  Bot,
  Brain,
  Command,
  FilePen,
  FileSymlink,
  FileText,
  Globe,
  Image,
  Search,
  TerminalSquare,
  Trash2,
  Wrench,
} from "../../../ui/leanIcons";

type ToolKind = ToolPresentation["kind"];

const toolKindIcons: Partial<Record<ToolKind | "extension", typeof FileText>> =
  {
    read: FileText,
    edit: FilePen,
    delete: Trash2,
    move: FileSymlink,
    search: Search,
    execute: TerminalSquare,
    fetch: Globe,
    webSearch: Command,
    imageView: Image,
    think: Brain,
    switchMode: ArrowRightLeft,
    mcp: Command,
    subagent: Bot,
  };

export function ToolKindIcon({ kind }: { kind: ToolKind | "extension" }) {
  const Icon = toolKindIcons[kind] ?? Wrench;
  return <Icon aria-hidden="true" size={14} data-tool-kind-icon={kind} />;
}
