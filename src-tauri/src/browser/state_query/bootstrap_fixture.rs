use super::super::BrowserTabs;
use std::sync::Mutex;

/// Uses the production document/metadata state machine without a Wry Webview.
pub(crate) struct BrowserDocumentFixture(Mutex<BrowserTabs<usize>>);

impl BrowserDocumentFixture {
    pub(crate) fn new() -> Self {
        let mut tabs = BrowserTabs::new();
        tabs.insert("browser-1".into(), 1, false);
        assert!(tabs.start_navigation("browser-1", 1, "https://example.com/"));
        assert!(tabs.finish_navigation("browser-1", 1, Some("https://example.com/")));
        Self(Mutex::new(tabs))
    }

    pub(crate) fn settled_tab(&self, label: &str) -> Result<(String, u64), String> {
        self.0.lock().unwrap().settled_tab(label)
    }

    pub(crate) fn late_metadata(&self) {
        let mut tabs = self.0.lock().unwrap();
        let url = "https://example.com/";
        let before = tabs.snapshot("browser-1").unwrap().revision;
        assert!(tabs.set_document_title("browser-1", url, "Late title"));
        let mut png = vec![0_u8; 45];
        png[..16].copy_from_slice(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR");
        png[16..20].copy_from_slice(&1_u32.to_be_bytes());
        png[20..24].copy_from_slice(&1_u32.to_be_bytes());
        png[33..].copy_from_slice(b"\0\0\0\0IEND\xaeB`\x82");
        assert!(tabs.set_favicon_png("browser-1", url, 1, Some(&png)));
        assert!(tabs.set_zoom_percent("browser-1", 125));
        assert!(tabs.snapshot("browser-1").unwrap().revision > before);
    }
}
