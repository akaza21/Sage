import { useState } from "react";
import { useWorkbenchStore } from "../../app/store";
import { errorMessage } from "../../domain/errors";
import { invokeNative } from "../../ports/nativeInvoke";
import { ConfirmDialog } from "../../ui/ConfirmDialog";
import { Notice } from "../../ui/Notice";
import { SettingsRow } from "./SettingsRow";

const DAY = 86_400_000;
const PRESETS = [30, 90, 180];
const chats = (count: number) => `${count} chat${count === 1 ? "" : "s"}`;

/** Deletes, in every project, the chats not used since a chosen point. */
export function AgentDataSettings() {
  const transitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const [days, setDays] = useState(90);
  const [confirm, setConfirm] = useState<{
    cutoff: number;
    count: number;
  } | null>(null);
  const [pending, setPending] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function run(work: () => Promise<void>) {
    setPending(true);
    setError(null);
    setMessage(null);
    try {
      await work();
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setPending(false);
    }
  }
  // The count fixes the cutoff, so the delete removes what was counted.
  const review = () =>
    run(async () => {
      const cutoff = Date.now() - days * DAY;
      const count = await invokeNative<number>("count_stale_chats", {
        cutoff,
      });
      if (count) setConfirm({ cutoff, count });
      else setMessage(`No chats older than ${days} days.`);
    });
  const remove = (cutoff: number) =>
    run(async () => {
      setConfirm(null);
      const actions = await import("../../app/chatArchiveActions");
      const { committed, stopped } = await actions.cleanUpProjectData(
        "delete_stale_chats",
        { cutoff },
      );
      // A cleanup that stopped part way reports why beside what it removed.
      if (stopped) setError(stopped);
      else if (committed) setMessage("Old chats deleted.");
      else setError(useWorkbenchStore.getState().error ?? "Nothing deleted.");
    });

  return (
    <>
      <p>Clear out chats you no longer use. Pinned chats are always kept.</p>
      <div className="agent-settings__card">
        <SettingsRow
          title="Delete old chats"
          description="Deletes chats in every project that have not been used in this long, with their history. Chats with a running turn are kept."
          notice={
            error ? (
              <Notice
                identity="settings:delete-old-chats"
                severity="warning"
                message={error}
              />
            ) : null
          }
        >
          <select
            className="select"
            aria-label="Delete chats older than"
            value={days}
            disabled={pending}
            onChange={(event) => setDays(Number(event.target.value))}
          >
            {PRESETS.map((preset) => (
              <option value={preset} key={preset}>
                Older than {preset} days
              </option>
            ))}
          </select>
          <button
            className="button button--compact"
            type="button"
            disabled={pending || transitioning}
            onClick={() => void review()}
          >
            Delete…
          </button>
        </SettingsRow>
      </div>
      {message ? <p role="status">{message}</p> : null}
      {confirm ? (
        <ConfirmDialog
          title={`Delete ${chats(confirm.count)}?`}
          description={`${chats(confirm.count)} not used in the last ${days} days will be permanently deleted from Sage with their history. This can't be undone.`}
          confirmLabel={`Delete ${chats(confirm.count)}`}
          onCancel={() => setConfirm(null)}
          onConfirm={() => void remove(confirm.cutoff)}
        />
      ) : null}
    </>
  );
}
