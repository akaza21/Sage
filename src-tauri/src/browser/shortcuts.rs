use serde::Serialize;
#[cfg(windows)]
use std::{sync::mpsc, time::Duration};
#[cfg(windows)]
use tauri::{AppHandle, Emitter, Manager, Webview};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum BrowserShortcut {
    FocusAddress,
    Reload,
    Back,
    Forward,
    Find,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    NewTab,
    CloseTab,
    Stop,
}

#[derive(Clone, Copy)]
pub(super) struct KeyChord {
    pub(super) key: u32,
    pub(super) ctrl: bool,
    pub(super) alt: bool,
    pub(super) shift: bool,
    pub(super) loading: bool,
}

pub(super) fn shortcut_for(chord: KeyChord) -> Option<BrowserShortcut> {
    if chord.ctrl && chord.shift && !chord.alt && chord.key == 0xBB {
        return Some(BrowserShortcut::ZoomIn);
    }
    if chord.ctrl && !chord.alt && !chord.shift {
        return match chord.key {
            0x4C => Some(BrowserShortcut::FocusAddress),
            0x52 => Some(BrowserShortcut::Reload),
            0x46 => Some(BrowserShortcut::Find),
            0x54 => Some(BrowserShortcut::NewTab),
            0x57 => Some(BrowserShortcut::CloseTab),
            0x30 => Some(BrowserShortcut::ZoomReset),
            0xBB | 0x6B => Some(BrowserShortcut::ZoomIn),
            0xBD | 0x6D => Some(BrowserShortcut::ZoomOut),
            _ => None,
        };
    }
    if chord.alt && !chord.ctrl && !chord.shift {
        return match chord.key {
            0x25 => Some(BrowserShortcut::Back),
            0x27 => Some(BrowserShortcut::Forward),
            _ => None,
        };
    }
    if !chord.ctrl && !chord.alt && !chord.shift {
        return match chord.key {
            0x74 => Some(BrowserShortcut::Reload),
            0x1B if chord.loading => Some(BrowserShortcut::Stop),
            _ => None,
        };
    }
    None
}

#[cfg(windows)]
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ShortcutIntent {
    tab_id: String,
    action: BrowserShortcut,
}

#[cfg(windows)]
pub(super) async fn register_native(app: AppHandle, webview: Webview) -> Result<(), String> {
    use webview2_com::AcceleratorKeyPressedEventHandler;
    use webview2_com_sys::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_KEY_EVENT_KIND, COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN,
        COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_MENU, VK_SHIFT};

    let label = webview.label().to_owned();
    let (sender, receiver) = mpsc::sync_channel(1);
    webview
        .with_webview(move |platform| {
            let result = {
                let controller = platform.controller();
                let callback = AcceleratorKeyPressedEventHandler::create(Box::new(
                    move |_controller, args| {
                        let Some(args) = args else {
                            return Ok(());
                        };
                        let mut kind = COREWEBVIEW2_KEY_EVENT_KIND::default();
                        let mut key = 0;
                        unsafe {
                            args.KeyEventKind(&mut kind)?;
                            args.VirtualKey(&mut key)?;
                        }
                        if kind != COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN
                            && kind != COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN
                        {
                            return Ok(());
                        }
                        let pressed =
                            |virtual_key: u16| unsafe { GetKeyState(i32::from(virtual_key)) < 0 };
                        let loading = app
                            .state::<super::BrowserState>()
                            .tabs
                            .lock()
                            .ok()
                            .and_then(|tabs| tabs.snapshot(&label))
                            .is_some_and(|snapshot| snapshot.loading);
                        let chord = KeyChord {
                            key,
                            ctrl: pressed(VK_CONTROL.0),
                            alt: pressed(VK_MENU.0),
                            shift: pressed(VK_SHIFT.0),
                            loading,
                        };
                        if let Some(action) = shortcut_for(chord) {
                            unsafe {
                                args.SetHandled(true)?;
                            }
                            let _ = app.emit_to(
                                "main",
                                "browser:shortcut",
                                ShortcutIntent {
                                    tab_id: label.clone(),
                                    action,
                                },
                            );
                        }
                        Ok(())
                    },
                ));
                let mut token = 0;
                unsafe { controller.add_AcceleratorKeyPressed(&callback, &mut token) }
                    .map_err(|error| error.to_string())
            };
            let _ = sender.send(result);
        })
        .map_err(|error| error.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        receiver
            .recv_timeout(Duration::from_secs(5))
            .map_err(|error| error.to_string())?
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(not(windows))]
pub(super) async fn register_native(
    _app: tauri::AppHandle,
    _webview: tauri::Webview,
) -> Result<(), String> {
    Err("native browser shortcuts are unavailable on this platform".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(key: u32) -> KeyChord {
        KeyChord {
            key,
            ctrl: false,
            alt: false,
            shift: false,
            loading: false,
        }
    }

    #[test]
    fn browser_accelerators_map_only_the_admitted_chords() {
        let mut ctrl = chord(b'L' as u32);
        ctrl.ctrl = true;
        assert_eq!(shortcut_for(ctrl), Some(BrowserShortcut::FocusAddress));
        for (key, expected) in [
            (b'R', BrowserShortcut::Reload),
            (b'F', BrowserShortcut::Find),
            (b'T', BrowserShortcut::NewTab),
            (b'W', BrowserShortcut::CloseTab),
            (b'0', BrowserShortcut::ZoomReset),
            (0xBB, BrowserShortcut::ZoomIn),
            (0xBD, BrowserShortcut::ZoomOut),
        ] {
            assert_eq!(
                shortcut_for(KeyChord {
                    key: key as u32,
                    ..ctrl
                }),
                Some(expected)
            );
        }
        assert_eq!(shortcut_for(chord(0x74)), Some(BrowserShortcut::Reload));
        assert_eq!(
            shortcut_for(KeyChord {
                key: 0xBB,
                shift: true,
                ..ctrl
            }),
            Some(BrowserShortcut::ZoomIn)
        );
        let alt = KeyChord {
            alt: true,
            ..chord(0x25)
        };
        assert_eq!(shortcut_for(alt), Some(BrowserShortcut::Back));
        assert_eq!(
            shortcut_for(KeyChord { key: 0x27, ..alt }),
            Some(BrowserShortcut::Forward)
        );
        assert_eq!(
            shortcut_for(KeyChord {
                loading: true,
                ..chord(0x1B)
            }),
            Some(BrowserShortcut::Stop)
        );
    }

    #[test]
    fn normal_page_typing_and_unrequested_modifier_combinations_pass_through() {
        assert_eq!(shortcut_for(chord(b'L' as u32)), None);
        assert_eq!(shortcut_for(chord(0x1B)), None);
        assert_eq!(
            shortcut_for(KeyChord {
                ctrl: true,
                alt: true,
                ..chord(b'L' as u32)
            }),
            None
        );
        assert_eq!(
            shortcut_for(KeyChord {
                ctrl: true,
                shift: true,
                ..chord(b'T' as u32)
            }),
            None
        );
    }
}
