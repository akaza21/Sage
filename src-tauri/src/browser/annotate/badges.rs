//! Bounded draft projections; these main-only chrome updates never capture.
use super::{
    BrowserState,
    element::{self, BrowserElement},
};
use tauri::{AppHandle, Manager};

#[cfg(windows)]
pub(crate) fn clear(app: &AppHandle, label: &str) {
    let app = app.clone();
    let label = label.to_owned();
    std::thread::spawn(move || {
        let state = app.state::<BrowserState>();
        let _ = state.annotate.run(
            |_| {},
            |_| {
                let result = state
                    .webview_for_tab(&label)
                    .ok_or("browser tab unavailable".into())
                    .and_then(|webview| {
                        crate::browser::devtools::set_annotation_badges(&webview, &[], "", false)
                    });
                (Vec::new(), result)
            },
        );
    });
}

pub(crate) async fn set(
    app: AppHandle,
    tab_id: String,
    annotations: Vec<BrowserElement>,
) -> Result<(), String> {
    admit(&tab_id, &annotations)?;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<BrowserState>();
        state.annotate.run(
            |_| {},
            |mode| {
                let result = (|| {
                    let url = state
                        .tabs
                        .lock()
                        .map_err(|_| "browser unavailable")?
                        .openable_url(&tab_id)?;
                    let annotations = current_annotations(annotations, &url);
                    let paused = mode.pending.is_some() && mode.tab.as_deref() == Some(&tab_id);
                    #[cfg(windows)]
                    {
                        let webview = state
                            .webview_for_tab(&tab_id)
                            .ok_or("browser tab unavailable")?;
                        crate::browser::devtools::set_annotation_badges(
                            &webview,
                            &annotations,
                            &url,
                            paused,
                        )
                    }
                    #[cfg(not(windows))]
                    {
                        let _ = (url, paused, annotations);
                        Ok(())
                    }
                })();
                (Vec::new(), result)
            },
        )
    })
    .await
    .map_err(|_| "browser badges unavailable".to_owned())?
}

fn admit(tab_id: &str, annotations: &[BrowserElement]) -> Result<(), String> {
    if annotations.len() > 8 {
        return Err("browser badge limit exceeded".into());
    }
    let mut ordinals = std::collections::HashSet::new();
    for item in annotations {
        element::validate(item)?;
        if item.tab_id != tab_id || !ordinals.insert(item.ordinal) {
            return Err("browser badge scope rejected".into());
        }
    }
    Ok(())
}

fn current_annotations(mut annotations: Vec<BrowserElement>, url: &str) -> Vec<BrowserElement> {
    let url = crate::acp::browser::redact_url(url);
    annotations.retain(|item| item.url == url);
    annotations
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draft_badges_require_bounded_unique_redacted_items_from_their_tab() {
        let selection = crate::browser::annotate::selection::tests::selection("browser-1");
        let item = element::confirm(&selection.picked, 1, "").unwrap();
        assert!(admit("browser-1", std::slice::from_ref(&item)).is_ok());
        assert!(admit("browser-2", std::slice::from_ref(&item)).is_err());
        assert!(admit("browser-1", &[item.clone(), item.clone()]).is_err());
        assert!(admit("browser-1", &vec![item.clone(); 9]).is_err());
        let mut forged = item;
        forged.url = "https://example.com/?token=private".into();
        assert!(admit("browser-1", &[forged]).is_err());
    }

    #[test]
    fn badges_match_redacted_urls_and_drop_snapshots_after_navigation() {
        let selection = crate::browser::annotate::selection::tests::selection("browser-1");
        let mut item = element::confirm(&selection.picked, 1, "").unwrap();
        let raw = "https://example.com/?token=private";
        item.url = crate::acp::browser::redact_url(raw);
        assert_eq!(
            current_annotations(vec![item.clone()], raw),
            vec![item.clone()]
        );
        assert!(current_annotations(vec![item], "https://example.com/next").is_empty());
    }
}
