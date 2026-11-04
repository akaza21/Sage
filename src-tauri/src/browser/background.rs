//! One per-WebView memory policy, driven by placement and window minimization.
//! Placement comes exclusively from PageVisibility; hidden pages remain live.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MemoryState {
    Active,
    Hidden,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MemoryTarget {
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
#[cfg(windows)]
pub(crate) use native::refresh;

#[cfg(not(windows))]
pub(crate) fn refresh(_: &tauri::Webview) {}

#[cfg(test)]
mod tests;
