//! One per-WebView memory policy, driven by placement and window minimization.
//! Placement comes exclusively from PageVisibility; hidden pages remain live.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MemoryState {
    Active,
    Hidden,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MemoryTarget {
    Low,
    Normal,
}

impl MemoryState {
    fn for_visibility(placed: bool, minimized: bool) -> Self {
        if placed && !minimized {
            Self::Active
        } else {
            Self::Hidden
        }
    }

    fn target(self) -> MemoryTarget {
        match self {
            Self::Active => MemoryTarget::Normal,
            Self::Hidden => MemoryTarget::Low,
        }
    }
}

#[cfg(windows)]
mod native;

/// Supplies fresh placement and window state on the WebView's thread, then
/// applies the policy's target. Non-browser WebViews have no placement.
pub(crate) trait MemoryTargetPort {
    fn update_memory_target(
        &self,
        policy: impl FnOnce(Option<bool>, bool) -> MemoryTarget + Send + 'static,
    );
}

pub(crate) fn refresh(webview: &impl MemoryTargetPort) {
    webview.update_memory_target(|placement, minimized| {
        MemoryState::for_visibility(placement.unwrap_or(true), minimized).target()
    });
}

#[cfg(not(windows))]
impl MemoryTargetPort for tauri::Webview {
    fn update_memory_target(
        &self,
        _: impl FnOnce(Option<bool>, bool) -> MemoryTarget + Send + 'static,
    ) {
    }
}
