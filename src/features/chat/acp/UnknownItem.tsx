import type { AcpItem } from "../../../domain/acp";

type UnknownProjectionItem = Extract<AcpItem, { kind: "unsupported" }>;

export function UnknownItem({ item }: { item: UnknownProjectionItem }) {
  return (
    <article className="acp-unknown" aria-label="Unsupported agent item">
      <header>
        <strong>Unsupported item</strong>
        <span role="status">Not available in this Sage version</span>
      </header>
      <p>{item.payload.summary}</p>
      <span className="acp-unknown__category">{item.payload.category}</span>
      {item.payload.truncated ? <span>Summary truncated</span> : null}
    </article>
  );
}
