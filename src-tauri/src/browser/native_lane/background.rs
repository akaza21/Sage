//! Observe suspension without waking the page through CDP.
use std::{sync::mpsc, time::Duration};
use tauri::Manager;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, ICoreWebView2_3, ICoreWebView2_19,
};
use windows_core::Interface;

use super::Lane;
use crate::acp::{
    browser::{BrowserScope, attachment::BrowserAttachments},
    contract::{SageChatId, SageProjectId},
};

/// Keeps the native capability cached after its real attachment is revoked.
pub struct AgentAttachment {
    _capability: crate::browser::devtools::NativeDevtools,
    registry: BrowserAttachments,
    project: SageProjectId,
    chat: SageChatId,
    tab: String,
}

impl AgentAttachment {
    pub fn detach(&self) -> bool {
        self.registry.detach(&self.project, &self.chat).is_some()
    }

    pub fn take_over(&self) -> bool {
        self.registry.take_over(&self.tab)
    }

    pub fn hand_back(&self) -> bool {
        self.registry.hand_back(&self.tab)
    }
}

impl Lane {
    /// DevTools is a native owned window; its parent must actually be on screen.
    pub fn show_browser_window(&self) -> Result<(), String> {
        let window = self
            .app
            .get_window("main")
            .ok_or("main window unavailable")?;
        window.show().map_err(|error| error.to_string())?;
        if !window.is_visible().map_err(|error| error.to_string())? {
            return Err("native browser parent is not visible".into());
        }
        Ok(())
    }

    pub fn agent_attachment(&self, tab: &str) -> Result<AgentAttachment, String> {
        let registry = BrowserAttachments::default();
        let project =
            SageProjectId::parse("project-native-background").map_err(|e| e.to_string())?;
        let chat = SageChatId::parse("chat-native-background").map_err(|e| e.to_string())?;
        let (url, revision) = self.state().settled_tab(tab)?;
        registry
            .attach(project.clone(), chat.clone(), tab, &url, revision)
            .map_err(|()| "attachment unavailable")?;
        let lease = registry
            .resolve(&project, &chat)
            .ok_or("attachment unavailable")?;
        let scope = BrowserScope {
            session_id: "native-background".into(),
            tab_id: lease.tab_number,
        };
        let capability =
            crate::browser::devtools::NativeDevtools::for_tab(&self.state(), scope, Some(lease))
                .map_err(|e| format!("attach: {e:?}"))?;
        Ok(AgentAttachment {
            _capability: capability,
            registry,
            project,
            chat,
            tab: tab.to_owned(),
        })
    }

    pub fn open_browser_devtools(&self, tab: &str) -> Result<(), String> {
        let webview = self
            .state()
            .webview_for_tab(tab)
            .ok_or("browser tab unavailable")?;
        let _awake = crate::browser::background::ensure_awake(&webview)?;
        let (sender, receiver) = mpsc::sync_channel(1);
        webview
            .with_webview(move |platform| {
                let result = unsafe {
                    platform
                        .controller()
                        .CoreWebView2()
                        .and_then(|core| core.OpenDevToolsWindow())
                }
                .map_err(|error| error.to_string());
                let _ = sender.send(result);
            })
            .map_err(|error| error.to_string())?;
        receiver
            .recv_timeout(Duration::from_secs(5))
            .map_err(|error| error.to_string())?
    }

    pub fn hide_browser(&self, tab: &str) -> Result<(), String> {
        tauri::async_runtime::block_on(crate::browser::page_controls::browser_hide(
            self.app.clone(),
            self.state(),
            tab.to_owned(),
            None,
        ))
    }

    pub fn browser_suspended(&self, tab: &str) -> Result<bool, String> {
        let webview = self
            .state()
            .webview_for_tab(tab)
            .ok_or("browser tab unavailable")?;
        let (sender, receiver) = mpsc::sync_channel(1);
        webview
            .with_webview(move |platform| {
                let result = (|| {
                    let core = unsafe { platform.controller().CoreWebView2() }?
                        .cast::<ICoreWebView2_3>()?;
                    let mut suspended = Default::default();
                    unsafe { core.IsSuspended(&mut suspended) }?;
                    Ok::<_, windows_core::Error>(suspended.as_bool())
                })()
                .map_err(|error| error.to_string());
                let _ = sender.send(result);
            })
            .map_err(|error| error.to_string())?;
        receiver
            .recv_timeout(Duration::from_secs(5))
            .map_err(|error| error.to_string())?
    }

    pub fn browser_memory_low(&self, tab: &str) -> Result<bool, String> {
        let webview = self
            .state()
            .webview_for_tab(tab)
            .ok_or("browser tab unavailable")?;
        let (sender, receiver) = mpsc::sync_channel(1);
        webview
            .with_webview(move |platform| {
                let result = (|| {
                    let core = unsafe { platform.controller().CoreWebView2() }?
                        .cast::<ICoreWebView2_19>()?;
                    let mut target = Default::default();
                    unsafe { core.MemoryUsageTargetLevel(&mut target) }?;
                    Ok::<_, windows_core::Error>(
                        target == COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
                    )
                })()
                .map_err(|error| error.to_string());
                let _ = sender.send(result);
            })
            .map_err(|error| error.to_string())?;
        receiver
            .recv_timeout(Duration::from_secs(5))
            .map_err(|error| error.to_string())?
    }
}
