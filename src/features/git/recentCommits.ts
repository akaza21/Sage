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

export function shortCommitSha(sha: string): string | null {
  const admitted = admitRecentCommitSha(sha);
  return admitted ? admitted.slice(0, SHORT_COMMIT_SHA_CHARS) : null;
}

export function visibleRecentCommits(
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
