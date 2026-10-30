import { type GitRecentCommit } from "../domain/workbench";

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

const MAX_RECENT_COMMITS = 64;

const SHORT_COMMIT_SHA_CHARS = 7;

const FULL_SHA = /^[0-9a-f]{40}$/u;

interface RecentCommit {
  sha: string;
  subject: string;
}

function admitRecentCommitSha(sha: string): string | null {
  const normalized = sha.trim().toLowerCase();
  return FULL_SHA.test(normalized) ? normalized : null;
}

function shortCommitSha(sha: string): string | null {
  const admitted = admitRecentCommitSha(sha);
  return admitted ? admitted.slice(0, SHORT_COMMIT_SHA_CHARS) : null;
}

function visibleRecentCommits(
  commits: readonly RecentCommit[] | undefined,
): RecentCommit[] {
  const visible: RecentCommit[] = [];
  for (const commit of commits ?? []) {
    const sha = admitRecentCommitSha(commit.sha);
    if (!sha) {
      continue;
    }
    visible.push({ sha, subject: commit.subject });
    if (visible.length === MAX_RECENT_COMMITS) {
      break;
    }
  }
  return visible;
}
