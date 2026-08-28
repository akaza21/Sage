import { FileKindIcon } from "../../files/FileKindIcon";
import { chatFileChipBasename, chatFileChipCount } from "./chatFileChip";

export function ChatFileChip({
  path,
  rootLabel = null,
  additions = null,
  deletions = null,
}: {
  path: string;
  rootLabel?: string | null;
  additions?: number | null;
  deletions?: number | null;
}) {
  const name = chatFileChipBasename(path);
  if (!name) return null;
  const additionsLabel = chatFileChipCount("+", additions);
  const deletionsLabel = chatFileChipCount("-", deletions);
  return (
    <span className="chat-file-chip">
      <FileKindIcon path={path} size={14} />
      <span className="chat-file-chip__name">{name}</span>
      {rootLabel ? (
        <span className="chat-file-chip__root">{rootLabel}</span>
      ) : null}
      {additionsLabel ? (
        <span className="tab__count tab__count--add">{additionsLabel}</span>
      ) : null}
      {deletionsLabel ? (
        <span className="tab__count tab__count--del">{deletionsLabel}</span>
      ) : null}
    </span>
  );
}
