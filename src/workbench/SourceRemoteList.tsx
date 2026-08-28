import type { RootRepositoryStatus } from "../domain/workbench";
import {
  canFetchRemote,
  canPushRemote,
  visibleRemotes,
} from "../features/git/remotes";

interface Props {
  rootLabel: string;
  remotes?: string[];
  truncated?: boolean;
  repository?: RootRepositoryStatus;
  transportDisabled?: boolean;
  onFetch?: (name: string) => void;
  onPush?: (name: string) => void;
}

export function SourceRemoteList({
  rootLabel,
  remotes,
  truncated = false,
  repository,
  transportDisabled = false,
  onFetch,
  onPush,
}: Props) {
  const names = visibleRemotes(remotes);
  if (names.length === 0 && !truncated) {
    return null;
  }
  return (
    <>
      {names.length > 0 ? (
        <>
          <p className="source-group-label">Remotes</p>
          <ul className="source-remotes" aria-label={`Remotes in ${rootLabel}`}>
            {names.map((name) => {
              const canFetch =
                repository !== undefined &&
                onFetch !== undefined &&
                canFetchRemote(repository, name);
              const canPush =
                repository !== undefined &&
                onPush !== undefined &&
                canPushRemote(repository, name);
              return (
                <li key={name} className="source-remote">
                  <span className="source-remote__name">{name}</span>
                  {canFetch || canPush ? (
                    <span className="source-remote__actions">
                      {canFetch ? (
                        <button
                          type="button"
                          className="source-remote__action"
                          disabled={transportDisabled}
                          onClick={() => onFetch(name)}
                        >
                          Fetch {name}
                        </button>
                      ) : null}
                      {canPush ? (
                        <button
                          type="button"
                          className="source-remote__action"
                          disabled={transportDisabled}
                          onClick={() => onPush(name)}
                        >
                          Push {name}
                        </button>
                      ) : null}
                    </span>
                  ) : null}
                </li>
              );
            })}
          </ul>
        </>
      ) : null}
      {truncated ? (
        <p className="tree__limit">Remote list capped at 16.</p>
      ) : null}
    </>
  );
}
