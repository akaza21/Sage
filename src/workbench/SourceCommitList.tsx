import { type GitRecentCommit } from "../domain/workbench";
import {
  shortCommitSha,
  visibleRecentCommits,
} from "../features/git/recentCommits";

interface Props {
  rootLabel: string;
  commits?: GitRecentCommit[];
  truncated?: boolean;
}

export function SourceCommitList({
  rootLabel,
  commits,
  truncated = false,
}: Props) {
  const names = visibleRecentCommits(commits);
  if (names.length === 0 && !truncated) {
    return null;
  }
  return (
    <>
      {names.length > 0 ? (
        <>
          <p className="source-group-label">Recent commits</p>
          <ul
            className="source-commits"
            aria-label={`Recent commits in ${rootLabel}`}
          >
            {names.map((commit) => {
              const short = shortCommitSha(commit.sha);
              if (!short) {
                return null;
              }
              return (
                <li key={commit.sha} className="source-commit">
                  <span className="source-commit__sha">{short}</span>
                  <span className="source-commit__subject">
                    {commit.subject || "Empty subject"}
                  </span>
                </li>
              );
            })}
          </ul>
        </>
      ) : null}
      {truncated ? (
        <p className="tree__limit">Commit list capped at 64.</p>
      ) : null}
    </>
  );
}
