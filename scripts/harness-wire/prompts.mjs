// The fixed prompts every harness is asked. Short replies keep transcripts
// small; each prompt names the behaviour the scenario exists to record.
export const PROMPTS = {
  hello: "Reply with the single word ok.",
  shell:
    "Use your shell tool twice. First run `git status --short` normally. Then run `sleep 3 && echo background-done` as a background command if your shell tool can run one in the background, otherwise normally. When both are done reply with the single word ok.",
  edit: "Append the line `bye` to the end of README.md by editing the file. Then reply with the single word ok.",
  image: "What two colours does this image show? Reply in five words or fewer.",
  subagent:
    "Delegate to a subagent: start one subagent whose only job is to reply with the word pong. When it has answered, reply with the single word ok.",
  plan: "Plan how to add a one-line CONTRIBUTING.md file to this repository. Keep the plan to one sentence, then present it for approval.",
  implement: "Implement the plan. Then reply with the single word ok.",
};
