//! Per-tab background policy. Placement comes exclusively from PageVisibility.

use std::time::{Duration, Instant};

impl super::BrowserState {
    /// Window restore must not overwrite a hidden tab's LOW target.
    #[cfg(windows)]
    pub(crate) fn refresh_tab_memory(&self, webview: &tauri::Webview) -> bool {
        if self.webview_for_tab(webview.label()).is_none() {
            return false;
        }
        refresh(webview);
        true
    }
}

pub(super) const SUSPEND_AFTER: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MemoryState {
    Active,
    Hidden { since: Instant },
    Suspended,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Pins {
    pub audio: bool,
    pub download: bool,
    pub devtools: bool,
    pub annotate: bool,
    pub agent: bool,
    pub loading: bool,
}

impl Pins {
    fn any(self) -> bool {
        self.audio || self.download || self.devtools || self.annotate || self.agent || self.loading
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MemoryTarget {
    Low,
    Normal,
}

impl MemoryTarget {
    fn for_placement(placed: bool, minimized: bool) -> Self {
        if placed && !minimized {
            Self::Normal
        } else {
            Self::Low
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Event {
    Changed,
    PinsChanged,
    Wake,
    Timer(u64),
    Completed(u64, bool),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Attempt {
    Ready,
    Pending,
    Declined,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Policy {
    pub state: MemoryState,
    pub generation: u64,
    attempt: Attempt,
    pins: Pins,
    target: MemoryTarget,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            state: MemoryState::Active,
            generation: 0,
            attempt: Attempt::Ready,
            pins: Pins::default(),
            target: MemoryTarget::Normal,
        }
    }
}

pub(super) struct Inputs {
    pub current: Policy,
    pub placed: bool,
    pub minimized: bool,
    pub pins: Pins,
    pub event: Event,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Transition {
    pub next: Policy,
    pub target: MemoryTarget,
    pub resume: bool,
    pub suspend: bool,
    pub sleep: Option<Duration>,
}

/// No native calls, clocks or tasks: stale callbacks have no effects.
pub(super) fn next_state(input: Inputs, now: Instant) -> Option<Transition> {
    let Inputs {
        current,
        placed,
        minimized,
        pins,
        event,
    } = input;
    if matches!(event, Event::Timer(g) | Event::Completed(g, _) if g != current.generation) {
        return None;
    }
    if matches!(event, Event::Completed(_, _)) && current.attempt != Attempt::Pending {
        return None;
    }
    let mut next = current;
    let mut suspend = false;
    let mut resume = false;
    if placed {
        next.state = MemoryState::Active;
        next.attempt = Attempt::Ready;
        resume = event == Event::Wake || current.state != MemoryState::Active;
    } else if let Event::Completed(_, successful) = event {
        if !successful {
            next.attempt = Attempt::Declined;
        } else {
            next.attempt = Attempt::Ready;
            if pins.any() {
                next.state = MemoryState::Hidden { since: now };
                resume = true;
            } else {
                next.state = MemoryState::Suspended;
            }
        }
    } else if matches!(current.state, MemoryState::Active)
        || event == Event::Wake
        || (matches!(current.state, MemoryState::Hidden { .. }) && event == Event::PinsChanged)
        || (current.state == MemoryState::Suspended && pins.any())
        || (pins != current.pins && current.state != MemoryState::Suspended)
    {
        next.state = MemoryState::Hidden { since: now };
        resume = event == Event::Wake
            || current.state == MemoryState::Suspended
            || current.attempt == Attempt::Pending;
        if matches!(current.state, MemoryState::Active | MemoryState::Suspended)
            || current.attempt == Attempt::Pending
        {
            next.attempt = Attempt::Ready;
        }
    } else if let MemoryState::Hidden { since } = current.state
        && current.attempt == Attempt::Ready
        && !pins.any()
        && now.saturating_duration_since(since) >= SUSPEND_AFTER
    {
        next.attempt = Attempt::Pending;
        suspend = true;
    }
    next.pins = pins;
    // Record the desired effect, not window-only input changes that cannot
    // change a hidden tab's LOW target or invalidate its pending completion.
    next.target = MemoryTarget::for_placement(placed, minimized);
    if matches!(event, Event::Completed(_, true)) && next.state != MemoryState::Suspended {
        resume = true;
    }
    if next == current && event != Event::Wake {
        return None;
    }
    next.generation = next.generation.wrapping_add(1);
    let sleep = match next.state {
        MemoryState::Hidden { since } if next.attempt == Attempt::Ready && !pins.any() => {
            Some(SUSPEND_AFTER.saturating_sub(now.saturating_duration_since(since)))
        }
        _ => None,
    };
    Some(Transition {
        next,
        target: next.target,
        resume,
        suspend,
        sleep,
    })
}

#[cfg(windows)]
mod native;
#[cfg(windows)]
pub(super) use native::{Background, ensure_awake, refresh, register};
#[cfg(windows)]
pub(super) use native::{SessionPin, pin_session};

#[cfg(not(windows))]
#[derive(Default)]
pub(super) struct Background {}
#[cfg(not(windows))]
pub(super) struct Awake {}
#[cfg(not(windows))]
pub(super) fn ensure_awake(_: &tauri::Webview) -> Result<Awake, String> {
    Ok(Awake {})
}
#[cfg(not(windows))]
pub(super) fn refresh(_: &tauri::Webview) {}

#[cfg(test)]
mod race_tests;
#[cfg(test)]
mod tests;
