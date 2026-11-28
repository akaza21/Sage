// Keeps one GitHub issue per failing After merge job. The workflow's last
// job runs this with the built-in token.
//
// Each run sets every issue from the same evidence: the last completed push
// and nightly runs on main, plus this run. For each job, the newest run that
// actually ran it (passed or failed; skipped and cancelled jobs say nothing)
// decides. A failure opens the job's issue or rewrites its body; a pass
// closes it with one comment. Any run, in any order, converges on the same
// issues, and a nightly-only job such as the packages keeps its last result
// across the push runs that skip it.
//
//   node scripts/ci-failure-issues.mjs
//
// Reads GH_TOKEN, GITHUB_REPOSITORY, GITHUB_RUN_ID, GITHUB_RUN_ATTEMPT and
// GITHUB_SERVER_URL from the Actions environment, plus REPORTER_NAME, the
// reporting job's own name, and calls `gh`.

import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

export const issueLabel = "after-merge";
// How many completed runs on main the reporter reads.
export const historyRuns = 20;
const failed = new Set(["failure", "timed_out"]);
const decisive = new Set([...failed, "success"]);
const reportingEvents = new Set(["push", "schedule"]);

export function issueTitle(jobName) {
  return `After merge: ${jobName} fails`;
}

// Each job's newest decisive result across `runs`, as a Map from job name to
// { failed, run, job, firstFailure }. `firstFailure` is the oldest run of the
// unbroken failing streak that ends at the newest result.
export function latestResults(runs, reporter) {
  const newestFirst = [...runs].sort((a, b) => b.run_number - a.run_number);
  const results = new Map();
  for (const run of newestFirst) {
    for (const job of run.jobs) {
      if (job.name === reporter || !decisive.has(job.conclusion)) continue;
      const result = results.get(job.name);
      if (!result) {
        results.set(job.name, {
          failed: failed.has(job.conclusion),
          run,
          job,
          firstFailure: failed.has(job.conclusion) ? run : undefined,
          streak: failed.has(job.conclusion),
        });
      } else if (result.streak) {
        if (failed.has(job.conclusion)) result.firstFailure = run;
        else result.streak = false;
      }
    }
  }
  return results;
}

export function issueBody(name, result) {
  const seen = (run) => `${run.sha} (${run.url})`;
  return [
    `**${name}** fails after merge.`,
    "",
    `- First seen: ${seen(result.firstFailure)}`,
    `- Last seen: ${seen(result.run)} (${result.run.event})`,
    `- Job log: ${result.job.html_url}`,
    "",
    "Fix it in a pull request that names this issue. The job's next passing run on main closes it.",
  ].join("\n");
}

// The issue changes that bring the label's open issues in line with
// `results`. An issue already in that state needs no change.
export function planIssueActions(results, openIssues) {
  const byTitle = new Map(openIssues.map((issue) => [issue.title, issue]));
  const actions = [];
  for (const [name, result] of [...results].sort(([a], [b]) =>
    a.localeCompare(b),
  )) {
    const title = issueTitle(name);
    const open = byTitle.get(title);
    if (result.failed) {
      const body = issueBody(name, result);
      if (!open) actions.push({ kind: "create", title, body });
      // GitHub may store the body with CRLF line ends.
      else if (open.body?.replaceAll("\r\n", "\n").trim() !== body) {
        actions.push({ kind: "edit", number: open.number, body });
      }
    } else if (open) {
      actions.push({
        kind: "close",
        number: open.number,
        body: `Passes at ${result.run.sha} in ${result.run.url} (${result.run.event}).`,
      });
    }
  }
  return actions;
}

function gh(args) {
  const result = spawnSync("gh", args, {
    encoding: "utf8",
    maxBuffer: 16 * 1024 * 1024,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`gh ${args[0]} ${args[1]} failed: ${result.stderr}`);
  }
  return result.stdout;
}

function runJobs(repo, path) {
  return gh([
    "api",
    "--paginate",
    path,
    "--jq",
    ".jobs[] | {name, conclusion, html_url}",
  ])
    .split("\n")
    .filter(Boolean)
    .map((line) => JSON.parse(line));
}

const runFields = "{id, run_number, event, sha: .head_sha, url: .html_url}";

function main(env = process.env) {
  const repo = env.GITHUB_REPOSITORY;
  const current = JSON.parse(
    gh([
      "api",
      `repos/${repo}/actions/runs/${env.GITHUB_RUN_ID}`,
      "--jq",
      `${runFields} + {workflow_id}`,
    ]),
  );
  current.jobs = runJobs(
    repo,
    `repos/${repo}/actions/runs/${current.id}/attempts/${env.GITHUB_RUN_ATTEMPT}/jobs`,
  );
  const completed = JSON.parse(
    gh([
      "api",
      `repos/${repo}/actions/workflows/${current.workflow_id}/runs?branch=main&status=completed&per_page=50`,
      "--jq",
      `[.workflow_runs[] | ${runFields}]`,
    ]),
  )
    .filter((run) => reportingEvents.has(run.event) && run.id !== current.id)
    .slice(0, historyRuns);
  for (const run of completed) {
    run.jobs = runJobs(repo, `repos/${repo}/actions/runs/${run.id}/jobs`);
  }

  const results = latestResults([current, ...completed], env.REPORTER_NAME);
  const openIssues = JSON.parse(
    gh([
      "issue",
      "list",
      "--repo",
      repo,
      "--label",
      issueLabel,
      "--state",
      "open",
      "--limit",
      "200",
      "--json",
      "number,title,body",
    ]),
  );
  const actions = planIssueActions(results, openIssues);
  if (actions.some((action) => action.kind === "create")) {
    gh([
      "label",
      "create",
      issueLabel,
      "--repo",
      repo,
      "--color",
      "B60205",
      "--description",
      "A check that runs after merge fails on main",
      "--force",
    ]);
  }
  for (const action of actions) {
    const number = String(action.number);
    if (action.kind === "create") {
      gh([
        "issue",
        "create",
        "--repo",
        repo,
        "--label",
        issueLabel,
        "--title",
        action.title,
        "--body",
        action.body,
      ]);
    } else if (action.kind === "edit") {
      gh(["issue", "edit", number, "--repo", repo, "--body", action.body]);
    } else {
      gh(["issue", "close", number, "--repo", repo, "--comment", action.body]);
    }
    console.log(`${action.kind} ${action.number ?? action.title}`);
  }
  if (actions.length === 0)
    console.log("issues already match the latest results");
}

if (process.argv[1] === fileURLToPath(import.meta.url)) main();
