import { isCurrentLocalBranch } from "../features/git/localBranches";

interface Props {
  rootLabel: string;
  branch: string | null;
  branches?: string[];
  truncated?: boolean;
}

export function SourceBranchList({
  rootLabel,
  branch,
  branches,
  truncated = false,
}: Props) {
  const names = branches ?? [];
  if (names.length === 0 && !truncated) {
    return null;
  }
  return (
    <>
      {names.length > 0 ? (
        <ul
          className="source-branches"
          aria-label={`Local branches in ${rootLabel}`}
        >
          {names.map((name) => {
            const current = isCurrentLocalBranch(name, branch);
            return (
              <li
                key={name}
                className={
                  current
                    ? "source-branch source-branch--current"
                    : "source-branch"
                }
              >
                {current ? `${name} (current)` : name}
              </li>
            );
          })}
        </ul>
      ) : null}
      {truncated ? (
        <p className="tree__limit">Branch list capped at 64 names.</p>
      ) : null}
    </>
  );
}
