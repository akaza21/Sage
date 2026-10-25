//! S8 on WebView2: the Agent command layer (`DevtoolsCapability<NativePort>`)
//! finds the main frame (target info, or the frame-tree fallback), makes its
//! isolated world without `Page.enable`, and reads and acts truthfully on a
//! page that overrides its own globals, without the page seeing it.

use std::time::Duration;

use sage_lib::{
    acp::browser::{
        BrowserAction, BrowserCommand, BrowserResult, ElementHandle, FillSecret, SnapshotNode,
        WaitCondition,
    },
    native_lane::{AgentTab, Lane},
};
use serde_json::{Value, json};

use crate::{
    report::{Report, sage_sources, wait_for},
    site::Site,
};

const FAKE: &str = "FAKE-PHRASE-9d1";
const TRUTH: &str = "hostile-truth-41";
const SECRET: &str = "S8-FILL-6be0";

pub fn run(lane: &Lane, site: &Site, report: &mut Report) {
    report.section("S8 Agent devtools on WebView2 (hostile page)");
    let url = site.url("/hostile.html");
    let tab = match lane.open_tab(Some(&url), false) {
        Ok(tab) => tab,
        Err(error) => {
            report.check("s8.open", false, error);
            return;
        }
    };
    let settled = wait_for(Duration::from_secs(15), || lane.settled_url(&tab));
    if !report.check(
        "s8.page-loaded",
        settled.as_deref() == Some(&url),
        format!("{settled:?}"),
    ) {
        return;
    }
    let mut steps = Timeline::default();
    steps.mark(lane, &tab, "load");
    frame_and_world(lane, report, &tab);
    steps.mark(lane, &tab, "raw-cdp");
    let mut agent = match lane.agent(&tab) {
        Ok(agent) => agent,
        Err(error) => {
            report.check("s8.attach", false, error);
            return;
        }
    };
    steps.mark(lane, &tab, "attach");
    let fake = wait_text(&mut agent, FAKE, 300);
    let truth = wait_text(&mut agent, TRUTH, 5_000);
    report.check(
        "s8.text-wait",
        fake == Ok(false) && truth == Ok(true),
        format!("fake phrase -> {fake:?}; real text -> {truth:?}"),
    );
    steps.mark(lane, &tab, "text-wait");
    let context = lane.page_context(&tab);
    let context_text = context.as_ref().map_or(String::new(), |context| {
        context["text"].as_str().unwrap_or_default().to_owned()
    });
    report.check(
        "s8.page-context",
        context.as_ref().is_ok_and(|context| {
            context["title"] == "Hostile page" && !context.to_string().contains("FAKE-")
        }) && context_text.starts_with(&format!("Selected text:\n{TRUTH}\n")),
        format!("{context:?}").chars().take(300).collect::<String>(),
    );
    steps.mark(lane, &tab, "page-context");
    act(&mut agent, report, &mut steps, lane, &tab);
    steps.mark(lane, &tab, "fill");
    let value = lane.evaluate(
        &tab,
        "window.__valueOf(document.getElementById('name'))",
        false,
    );
    report.check(
        "s8.fill-value",
        value == Ok(json!(SECRET)),
        format!("the input holds {value:?}"),
    );
    steps.mark(lane, &tab, "fill-value-read");
    let seen = lane.evaluate(&tab, "window.__agentSeen", false);
    let globals = lane.evaluate(
        &tab,
        "Object.getOwnPropertyNames(window).length - window.__globalsAtLoad",
        false,
    );
    let stacks = lane.evaluate(&tab, "window.__agentStacks", false);
    report.check(
        "s8.unobserved",
        steps.no_new_hits()
            && steps.load_hits_are_tauri_devtools_only(&stacks)
            && globals == Ok(json!(0)),
        format!(
            "hook hits after each step (baseline at load) {}; page hooks saw {seen:?}; new page globals {globals:?}; caller stacks {stacks:?}",
            steps.describe(),
        ),
    );
}

/// How many page hooks had fired after each step. Hits before the first
/// mark (load) are the page's own load (in debug builds, Tauri's devtools
/// hotkey init script runs `JSON.parse` at `DOMContentLoaded`); every later
/// step must add none.
#[derive(Default)]
struct Timeline(Vec<(String, Result<Value, String>)>);

impl Timeline {
    fn mark(&mut self, lane: &Lane, tab: &str, step: &str) {
        let count = lane.evaluate(tab, "window.__agentSeen.length", false);
        self.0.push((step.to_owned(), count));
    }

    fn no_new_hits(&self) -> bool {
        let Some((_, Ok(baseline))) = self.0.first() else {
            return false;
        };
        self.0
            .iter()
            .all(|(_, count)| count.as_ref() == Ok(baseline))
    }

    /// The load baseline is zero, or, in a debug build only, exactly one hit:
    /// the page's `JSON.parse` hook called directly by Tauri's devtools
    /// hotkey init script. That script runs once per document and exists
    /// only under `debug_assertions` or tauri's `devtools` feature
    /// (`tauri` `webview/plugin.rs`), which Sage's release builds do not
    /// enable. Any other load-time hit fails the check.
    fn load_hits_are_tauri_devtools_only(&self, stacks: &Result<Value, String>) -> bool {
        let Some((_, Ok(baseline))) = self.0.first() else {
            return false;
        };
        match (baseline.as_u64(), stacks) {
            (Some(0), _) => true,
            (Some(1), Ok(Value::Array(stacks))) if cfg!(debug_assertions) => {
                matches!(stacks.as_slice(), [stack] if stack.as_str().is_some_and(devtools_hotkey_parse))
            }
            _ => false,
        }
    }

    fn describe(&self) -> String {
        self.0
            .iter()
            .map(|(step, count)| match count {
                Ok(count) => format!("{step}={count}"),
                Err(error) => format!("{step}={error}"),
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// `hostile.html`'s record of one hit, `JSON.parse: Error` then V8 frames:
/// only the hook's own frames in `hostile.html`, ending with the hook
/// itself (`at JSON.<anonymous> (…hostile.html:…)`), whose direct caller is
/// line 7 of Tauri's script (`at toggleDevtoolsHotkey (<anonymous>:7:…)`).
fn devtools_hotkey_parse(stack: &str) -> bool {
    let mut lines = stack.lines().map(str::trim);
    if lines.next() != Some("JSON.parse: Error") {
        return false;
    }
    let frames: Vec<&str> = lines.collect();
    let Some(hook) = frames.iter().position(|frame| {
        frame.starts_with("at JSON.<anonymous> (") && frame.contains("hostile.html:")
    }) else {
        return false;
    };
    frames[..hook]
        .iter()
        .all(|frame| frame.contains("hostile.html:"))
        && frames
            .get(hook + 1)
            .is_some_and(|caller| caller.starts_with("at toggleDevtoolsHotkey (<anonymous>:7:"))
}

/// The raw CDP steps the Agent world rests on, on this WebView2 build.
fn frame_and_world(lane: &Lane, report: &mut Report, tab: &str) {
    let info = lane.cdp(tab, "Target.getTargetInfo", &json!({}));
    let frame = match &info {
        Ok(info) => info["targetInfo"]["targetId"].as_str().map(str::to_owned),
        Err(_) => lane
            .cdp(tab, "Page.getFrameTree", &json!({}))
            .ok()
            .and_then(|tree| tree["frameTree"]["frame"]["id"].as_str().map(str::to_owned)),
    };
    report.check(
        "s8.main-frame",
        frame.is_some(),
        format!(
            "Target.getTargetInfo {}; frame id {frame:?}",
            match &info {
                Ok(_) => "answered".to_owned(),
                Err(error) => format!("refused ({error}), frame tree used"),
            }
        ),
    );
    let Some(frame) = frame else { return };
    let world = lane.cdp(
        tab,
        "Page.createIsolatedWorld",
        &json!({"frameId": frame, "worldName": "native-lane-probe"}),
    );
    let enabling: Vec<_> = sage_sources()
        .into_iter()
        .filter(|(_, source)| source.contains("\"Page.enable\""))
        .map(|(file, _)| file.display().to_string())
        .collect();
    report.check(
        "s8.isolated-world-without-page-enable",
        enabling.is_empty()
            && world
                .as_ref()
                .is_ok_and(|world| world["executionContextId"].as_u64().is_some()),
        format!("Sage sources sending Page.enable: {enabling:?}; createIsolatedWorld -> {world:?}"),
    );
}

fn act(agent: &mut AgentTab, report: &mut Report, steps: &mut Timeline, lane: &Lane, tab: &str) {
    let nodes = match agent.perform(BrowserCommand::SnapshotPage {}) {
        Ok(BrowserResult::SnapshotPage { nodes, .. }) => nodes,
        other => {
            report.check("s8.snapshot", false, format!("{other:?}"));
            return;
        }
    };
    let (Some(button), Some(name)) = (
        handle(&nodes, "button", "Press me"),
        handle(&nodes, "textbox", "Name"),
    ) else {
        report.check(
            "s8.snapshot",
            false,
            format!(
                "no Press me button or Name textbox among {} nodes",
                nodes.len()
            ),
        );
        return;
    };
    let click = agent.perform(BrowserCommand::Act {
        action: BrowserAction::Click { handle: button },
    });
    let pressed = wait_text(agent, "Pressed for real", 5_000);
    report.check(
        "s8.click",
        click == Ok(BrowserResult::Act { applied: true }) && pressed == Ok(true),
        format!("click -> {click:?}; real handler ran -> {pressed:?}"),
    );
    steps.mark(lane, tab, "click");
    let fill = agent.perform(BrowserCommand::Act {
        action: BrowserAction::Fill {
            handle: name,
            value: FillSecret::new(SECRET.into()),
        },
    });
    let filled = wait_text(agent, "name holds 12 characters", 5_000);
    report.check(
        "s8.fill",
        fill == Ok(BrowserResult::Act { applied: true }) && filled == Ok(true),
        format!("fill -> {fill:?}; input event saw the value -> {filled:?}"),
    );
}

fn wait_text(agent: &mut AgentTab, text: &str, timeout_ms: u32) -> Result<bool, String> {
    match agent.perform(BrowserCommand::WaitFor {
        condition: WaitCondition::Text { text: text.into() },
        timeout_ms,
    }) {
        Ok(BrowserResult::WaitFor { matched }) => Ok(matched),
        other => Err(format!("{other:?}")),
    }
}

fn handle(nodes: &[SnapshotNode], role: &str, name: &str) -> Option<ElementHandle> {
    nodes
        .iter()
        .find(|node| node.role == role && node.name == name)
        .map(|node| node.handle.clone())
}
