import { dayTimeLabel } from "../chatTimestamp";

export function TurnDaySeparator({ timestamp }: { timestamp: number | null }) {
  if (timestamp === null) return null;
  return (
    <div className="chat-transcript-day sr-only" role="separator">
      <time dateTime={new Date(timestamp).toISOString()}>
        {dayTimeLabel(timestamp)}
      </time>
    </div>
  );
}
