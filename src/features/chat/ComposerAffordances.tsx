import { AtSign, Paperclip, Slash } from "lucide-react";

const AFFORDANCES = [
  {
    Icon: Paperclip,
    label: "Attach files",
    reason: "File attachments are not available in this composer.",
  },
  {
    Icon: AtSign,
    label: "Mention context",
    reason: "Mentions are not available in this composer.",
  },
  {
    Icon: Slash,
    label: "Insert slash command",
    reason: "Slash commands are not available in this composer.",
  },
] as const;

export function ComposerAffordances() {
  return (
    <div className="composer__affordances" aria-label="Composer actions">
      {AFFORDANCES.map(({ Icon, label, reason }) => (
        <button
          className="composer__affordance"
          type="button"
          key={label}
          aria-label={`${label}: ${reason}`}
          title={reason}
          disabled
        >
          <Icon size={16} aria-hidden="true" />
        </button>
      ))}
    </div>
  );
}
