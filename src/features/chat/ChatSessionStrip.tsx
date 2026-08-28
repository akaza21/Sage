export function ChatSessionStrip({ title }: { title: string | null }) {
  if (!title) return null;
  return (
    <div className="session-tabs" role="tablist" aria-label="Session">
      <span className="session-tab" role="tab" aria-selected="true">
        {title}
      </span>
    </div>
  );
}
