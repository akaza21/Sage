import type { AcpItem } from "../../../domain/acp";

type CollaborationProjectionItem = Extract<AcpItem, { kind: "collaboration" }>;

export function CollaborationItem({
  item,
}: {
  item: CollaborationProjectionItem;
}) {
  return (
    <article className="acp-collaboration" aria-label="Harness collaboration">
      <strong>{item.payload.label}</strong>
      <p>{item.payload.summary}</p>
      {item.payload.receiverLabels.length > 0 ? (
        <p className="acp-collaboration__receivers">
          Participants: {item.payload.receiverLabels.join(", ")}
        </p>
      ) : null}
    </article>
  );
}
