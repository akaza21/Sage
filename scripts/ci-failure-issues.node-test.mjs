import assert from "node:assert/strict";
import test from "node:test";

import {
  issueBody,
  issueTitle,
  latestResults,
  planIssueActions,
} from "./ci-failure-issues.mjs";

const reporter = "Report failures";
const rust = "Rust / Rust (windows-latest)";
const packages = "Desktop packages / Windows installer";

// A completed run: `outcomes` maps job names to conclusions.
const run = (run_number, event, outcomes) => ({
  run_number,
  event,
  sha: `sha${run_number}`,
  url: `https://github.com/o/r/actions/runs/${run_number}`,
  jobs: [
    ...Object.entries(outcomes).map(([name, conclusion]) => ({
      name,
      conclusion,
      html_url: `https://github.com/o/r/actions/runs/${run_number}/job/${name.length}`,
    })),
    { name: reporter, conclusion: null, html_url: "" },
  ],
});
const actions = (runs, openIssues = []) =>
  planIssueActions(latestResults(runs, reporter), openIssues);
const summary = (list) =>
  list.map(({ kind, title, number }) => [kind, title ?? number]);

test("a failure opens one issue naming its first and latest failing runs", () => {
  const runs = [
    run(3, "push", { [rust]: "failure" }),
    run(2, "push", { [rust]: "timed_out" }),
    run(1, "push", { [rust]: "success" }),
  ];
  const [create] = actions(runs);
  assert.deepEqual(summary([create]), [["create", issueTitle(rust)]]);
  assert.match(create.body, /- First seen: sha2 \(.*\/runs\/2\)/);
  assert.match(create.body, /- Last seen: sha3 \(.*\/runs\/3\) \(push\)/);
  assert.match(create.body, /\/runs\/3\/job\//);
});

test("the newest result rewrites an open issue once, and then leaves it", () => {
  const runs = [
    run(4, "push", { [rust]: "failure" }),
    run(3, "push", { [rust]: "failure" }),
  ];
  const stale = { number: 7, title: issueTitle(rust), body: "older state" };
  const [edit] = actions(runs, [stale]);
  assert.deepEqual(summary([edit]), [["edit", 7]]);
  // A body already in the newest state needs no further change, so repeated
  // failures add nothing.
  assert.deepEqual(actions(runs, [{ ...stale, body: edit.body }]), []);
});

test("a pass closes the issue with one comment", () => {
  const runs = [
    run(5, "push", { [rust]: "success" }),
    run(4, "push", { [rust]: "failure" }),
  ];
  const open = [{ number: 7, title: issueTitle(rust), body: "" }];
  const [close] = actions(runs, open);
  assert.deepEqual(summary([close]), [["close", 7]]);
  assert.match(close.body, /sha5/);
});

test("any order of runs converges on the same issues", () => {
  const runs = [
    run(10, "push", { [rust]: "success", [packages]: "skipped" }),
    run(9, "schedule", { [rust]: "failure", [packages]: "failure" }),
    run(8, "push", { [rust]: "cancelled", [packages]: "skipped" }),
    run(7, "schedule", { [rust]: "failure", [packages]: "success" }),
  ];
  const open = [{ number: 1, title: issueTitle(rust), body: "" }];
  const expected = actions(runs, open);
  for (const order of [
    [...runs].reverse(),
    [runs[2], runs[0], runs[3], runs[1]],
  ]) {
    assert.deepEqual(actions(order, open), expected);
  }
  // Run 10 passed Rust; the cancelled run 8 says nothing.
  assert.deepEqual(summary(expected), [
    ["create", issueTitle(packages)],
    ["close", 1],
  ]);
});

test("push runs that skip the packages keep the nightly packages result", () => {
  const runs = [
    run(12, "push", { [rust]: "success", [packages]: "skipped" }),
    run(11, "push", { [rust]: "success", [packages]: "skipped" }),
    run(10, "schedule", { [rust]: "success", [packages]: "failure" }),
  ];
  const results = latestResults(runs, reporter);
  assert.equal(results.get(packages).failed, true);
  assert.equal(results.get(packages).run.run_number, 10);
  const body = issueBody(packages, results.get(packages));
  const open = [{ number: 3, title: issueTitle(packages), body }];
  assert.deepEqual(actions(runs, open), []);
});
