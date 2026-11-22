import { useEffect, useId, useState, type FormEvent } from "react";
import { errorMessage } from "../../domain/errors";
import { invokeNative, isTauriRuntime } from "../../ports/nativeInvoke";
import { SettingsRow } from "./SettingsRow";

/** A user-defined ACP agent as Rust stores it (names only, never values). */
export interface CustomAcpAgent {
  id: string;
  name: string;
  executable: string;
  args: string[];
  env: string[];
}

/** Rust admits at most this many; the form stops offering more. */
export const MAX_CUSTOM_AGENTS = 4;

interface Draft {
  id: string | null;
  name: string;
  executable: string;
  args: string;
  env: string;
}

const EMPTY: Draft = { id: null, name: "", executable: "", args: "", env: "" };

/** One argument per line: Sage never splits or runs them through a shell. */
export function draftArguments(text: string): string[] {
  return text
    .split(/\r?\n/u)
    .map((line) => line.trim())
    .filter(Boolean);
}

export function draftEnvironmentNames(text: string): string[] {
  return text
    .split(/[\s,]+/u)
    .map((name) => name.trim())
    .filter(Boolean);
}

export default function CustomAgentSettings() {
  const [agents, setAgents] = useState<CustomAcpAgent[]>([]);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const argumentsHintId = useId();
  const native = isTauriRuntime();

  useEffect(() => {
    if (!native) return;
    let active = true;
    invokeNative<CustomAcpAgent[]>("custom_acp_agents").then(
      (saved) => active && setAgents(saved.slice(0, MAX_CUSTOM_AGENTS)),
      (cause) => active && setError(errorMessage(cause)),
    );
    return () => {
      active = false;
    };
  }, [native]);

  async function run(command: string, args: Record<string, unknown>) {
    setBusy(true);
    setError(null);
    try {
      const saved = await invokeNative<CustomAcpAgent[]>(command, args);
      setAgents(saved.slice(0, MAX_CUSTOM_AGENTS));
      setDraft(null);
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setBusy(false);
    }
  }

  function submit(event: FormEvent) {
    event.preventDefault();
    if (!draft) return;
    void run("save_custom_acp_agent", {
      agent: {
        id: draft.id,
        name: draft.name.trim(),
        executable: draft.executable.trim(),
        args: draftArguments(draft.args),
        env: draftEnvironmentNames(draft.env),
      },
    });
  }

  if (!native) return null;
  return (
    <section className="settings-surface__group" aria-label="Your ACP agents">
      <h3>Your ACP agents</h3>
      <p>
        Add any agent that speaks the Agent Client Protocol over stdio. Sage
        runs the executable directly, never through a shell; on Windows it must
        be a native .exe file. List only the names of environment variables to
        pass through; their values stay in your environment.
      </p>
      {agents.map((agent) => (
        <SettingsRow
          key={agent.id}
          title={agent.name}
          description={[agent.executable, ...agent.args].join(" ")}
        >
          <button
            className="button button--compact"
            type="button"
            disabled={busy}
            onClick={() =>
              setDraft({
                id: agent.id,
                name: agent.name,
                executable: agent.executable,
                args: agent.args.join("\n"),
                env: agent.env.join(", "),
              })
            }
          >
            Edit
          </button>
          <button
            className="button button--compact"
            type="button"
            disabled={busy}
            onClick={() =>
              void run("remove_custom_acp_agent", { id: agent.id })
            }
          >
            Remove
          </button>
        </SettingsRow>
      ))}
      {draft ? (
        <form className="agent-settings__card" onSubmit={submit}>
          <label>
            Name
            <input
              value={draft.name}
              maxLength={64}
              required
              onChange={(event) =>
                setDraft({ ...draft, name: event.target.value })
              }
            />
          </label>
          <label>
            Executable path
            <input
              value={draft.executable}
              maxLength={1024}
              required
              onChange={(event) =>
                setDraft({ ...draft, executable: event.target.value })
              }
            />
          </label>
          <label>
            Arguments (one per line)
            <textarea
              value={draft.args}
              rows={3}
              aria-describedby={argumentsHintId}
              onChange={(event) =>
                setDraft({ ...draft, args: event.target.value })
              }
            />
          </label>
          <p id={argumentsHintId}>
            Arguments are saved and shown in plain text. Do not put API keys or
            tokens here: set them as environment variables and list their names
            below.
          </p>
          <label>
            Environment variable names to pass through
            <input
              value={draft.env}
              onChange={(event) =>
                setDraft({ ...draft, env: event.target.value })
              }
            />
          </label>
          <button className="button button--compact" disabled={busy}>
            {busy ? "Saving..." : "Save agent"}
          </button>
          <button
            className="button button--compact"
            type="button"
            disabled={busy}
            onClick={() => setDraft(null)}
          >
            Cancel
          </button>
        </form>
      ) : agents.length < MAX_CUSTOM_AGENTS ? (
        <div>
          <button
            className="button button--compact"
            type="button"
            disabled={busy}
            onClick={() => setDraft(EMPTY)}
          >
            Add ACP agent
          </button>
        </div>
      ) : (
        <p role="status">
          Sage holds up to {MAX_CUSTOM_AGENTS} agents. Remove one to add
          another.
        </p>
      )}
      {error ? <p role="alert">{error}</p> : null}
    </section>
  );
}
