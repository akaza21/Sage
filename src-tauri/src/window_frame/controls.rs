use std::cell::Cell;

use tauri::{AppHandle, Manager, Webview, WindowEvent};
use windows::{
    Win32::{
        Foundation::{
            GetLastError, HWND, LPARAM, LRESULT, POINT, RECT, SetLastError, WIN32_ERROR, WPARAM,
        },
        Graphics::Gdi::{HDC, InvalidateRect, PtInRect, ScreenToClient},
        UI::{
            Controls::WM_MOUSELEAVE,
            Input::KeyboardAndMouse::{
                GetCapture, ReleaseCapture, SetCapture, TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT,
                TrackMouseEvent,
            },
            Shell::{DefSubclassProc, GetWindowSubclass, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::{
                BM_CLICK, BS_FLAT, CreateWindowExW, DestroyWindow, GWL_STYLE, GetClientRect,
                GetCursorPos, GetDlgItem, GetWindowLongW, HMENU, HTBOTTOMRIGHT, HTCLOSE, HTLEFT,
                HTMAXBUTTON, HTMINBUTTON, HTTRANSPARENT, HWND_TOP, IsIconic, IsZoomed,
                PostMessageW, SC_MAXIMIZE, SC_MINIMIZE, SC_RESTORE, SW_HIDE, SWP_NOACTIVATE,
                SWP_SHOWWINDOW, SendMessageW, SetWindowLongW, SetWindowPos, SetWindowTextW,
                ShowWindow, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CANCELMODE, WM_CAPTURECHANGED,
                WM_CLOSE, WM_ERASEBKGND, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCDESTROY,
                WM_NCHITTEST, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP, WM_NCMOUSELEAVE, WM_PAINT,
                WM_PRINT, WM_PRINTCLIENT, WM_SYSCOMMAND, WS_CHILD, WS_CLIPSIBLINGS,
            },
        },
    },
    core::{PCWSTR, w},
};

use super::paint::{self, CaptionPalette};

const FIRST_CONTROL_ID: i32 = 0x5347;
const SUBCLASS_ID: usize = 1;
const BUTTONS: [Action; 3] = [Action::Minimize, Action::Maximize, Action::Close];

#[derive(Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CaptionLayout {
    right_inset: i32,
    top: i32,
    width: i32,
    height: i32,
    palette: CaptionPalette,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Minimize,
    Maximize,
    Close,
}

impl Action {
    unsafe fn label(self, parent: HWND) -> PCWSTR {
        match self {
            Self::Minimize => w!("Minimize window"),
            Self::Maximize if unsafe { IsZoomed(parent) }.as_bool() => w!("Restore window"),
            Self::Maximize => w!("Maximize window"),
            Self::Close => w!("Close window"),
        }
    }

    unsafe fn glyph(self, parent: HWND) -> u16 {
        match self {
            Self::Minimize => 0xE921,
            Self::Maximize if unsafe { IsZoomed(parent) }.as_bool() => 0xE923,
            Self::Maximize => 0xE922,
            Self::Close => 0xE8BB,
        }
    }

    fn hit_test(self) -> u32 {
        match self {
            Self::Minimize => HTMINBUTTON,
            Self::Maximize => HTMAXBUTTON,
            Self::Close => HTCLOSE,
        }
    }

    unsafe fn activate(self, parent: HWND) {
        unsafe {
            let (message, command) = match self {
                Self::Minimize => (WM_SYSCOMMAND, SC_MINIMIZE),
                Self::Maximize if IsZoomed(parent).as_bool() => (WM_SYSCOMMAND, SC_RESTORE),
                Self::Maximize => (WM_SYSCOMMAND, SC_MAXIMIZE),
                Self::Close => (WM_CLOSE, 0),
            };
            // Keep close-request handling and ACP shutdown on Tao's normal
            // event path, without re-entering a parent window procedure.
            let _ = PostMessageW(Some(parent), message, WPARAM(command as usize), LPARAM(0));
        }
    }
}

struct CaptionButton {
    parent: HWND,
    action: Action,
    index: usize,
    // SetWindowPos/SetWindowText synchronously re-enter this window proc.
    // Cells avoid holding a mutable Rust reference across those calls.
    layout: Cell<Option<CaptionLayout>>,
    hover: Cell<bool>,
    armed: Cell<bool>,
}

pub(super) async fn set_layout(
    webview: Webview,
    layout: Option<CaptionLayout>,
) -> Result<(), String> {
    if let Some(layout) = layout
        && (layout.right_inset < 0
            || layout.top < 0
            || layout.width < BUTTONS.len() as i32
            || layout.height <= 0
            || layout.palette.font_height <= 0
            || layout.palette.font_height > layout.height)
    {
        return Err("Invalid caption layout".into());
    }
    let window = webview.window();
    if webview.label() != "main" || window.is_decorated().map_err(|e| e.to_string())? {
        return Err("Custom captions require the undecorated workbench".into());
    }
    let parent = window.hwnd().map_err(|e| e.to_string())?.0 as usize;
    let (send, receive) = tokio::sync::oneshot::channel();
    webview
        .with_webview(move |platform| {
            // HWND and subclass state are accessed only on their owning GUI thread.
            let result = unsafe {
                (|| {
                    let mut host = HWND::default();
                    platform.controller().ParentWindow(&mut host)?;
                    // Wry's accelerated host must clip siblings or it paints
                    // over caption HWNDs even when they precede it in Z order.
                    let style = GetWindowLongW(host, GWL_STYLE);
                    if style & WS_CLIPSIBLINGS.0 as i32 == 0 {
                        SetLastError(WIN32_ERROR(0));
                        if SetWindowLongW(host, GWL_STYLE, style | WS_CLIPSIBLINGS.0 as i32) == 0 {
                            GetLastError().ok()?;
                        }
                    }
                    update(HWND(parent as _), layout)
                })()
            }
            .map_err(|e| e.to_string());
            let _ = send.send(result);
        })
        .map_err(|e| e.to_string())?;
    receive.await.map_err(|e| e.to_string())?
}

pub(crate) fn handle_window_event(app: &AppHandle, label: &str, event: &WindowEvent) {
    if label != "main"
        || !matches!(
            event,
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. }
        )
    {
        return;
    }
    let Some(window) = app.get_window(label) else {
        return;
    };
    let Ok(parent) = window.hwnd() else {
        return;
    };
    unsafe {
        for index in 0..BUTTONS.len() {
            if let Ok(child) = GetDlgItem(Some(parent), FIRST_CONTROL_ID + index as i32)
                && let Some(state) = state(child)
            {
                let _ = arrange(child, &*state);
            }
        }
    }
}

unsafe fn state(child: HWND) -> Option<*const CaptionButton> {
    let mut data = 0;
    unsafe { GetWindowSubclass(child, Some(subclass), SUBCLASS_ID, Some(&mut data)) }
        .as_bool()
        .then_some(data as *const CaptionButton)
}

unsafe fn update(parent: HWND, layout: Option<CaptionLayout>) -> windows::core::Result<()> {
    unsafe {
        for (index, action) in BUTTONS.into_iter().enumerate() {
            let id = FIRST_CONTROL_ID + index as i32;
            let child = match GetDlgItem(Some(parent), id) {
                Ok(child) => child,
                Err(_) if layout.is_none() => continue,
                Err(_) => {
                    // Standard BUTTON windows retain native accessible names,
                    // roles and Invoke semantics through their original proc.
                    let child = CreateWindowExW(
                        WINDOW_EX_STYLE(0),
                        w!("BUTTON"),
                        action.label(parent),
                        WS_CHILD | WINDOW_STYLE(BS_FLAT as u32),
                        0,
                        0,
                        0,
                        0,
                        Some(parent),
                        Some(HMENU(id as usize as _)),
                        None,
                        None,
                    )?;
                    let data = Box::into_raw(Box::new(CaptionButton {
                        parent,
                        action,
                        index,
                        layout: Cell::new(layout),
                        hover: Cell::new(false),
                        armed: Cell::new(false),
                    }));
                    if !SetWindowSubclass(child, Some(subclass), SUBCLASS_ID, data as usize)
                        .as_bool()
                    {
                        drop(Box::from_raw(data));
                        let _ = DestroyWindow(child);
                        return Err(windows::core::Error::from_win32());
                    }
                    child
                }
            };
            if let Some(data) = state(child) {
                (*data).layout.set(layout);
                arrange(child, &*data)?;
            }
        }
        Ok(())
    }
}

unsafe fn arrange(child: HWND, button: &CaptionButton) -> windows::core::Result<()> {
    unsafe {
        let mut client = RECT::default();
        GetClientRect(button.parent, &mut client)?;
        let geometry = button.layout.get().and_then(|layout| {
            let left = client
                .right
                .checked_sub(layout.right_inset)?
                .checked_sub(layout.width)?;
            let bottom = layout.top.checked_add(layout.height)?;
            if left < 0 || bottom > client.bottom || IsIconic(button.parent).as_bool() {
                return None;
            }
            let start =
                (i64::from(layout.width) * button.index as i64 / BUTTONS.len() as i64) as i32;
            let end =
                (i64::from(layout.width) * (button.index + 1) as i64 / BUTTONS.len() as i64) as i32;
            Some((left + start, layout.top, end - start, layout.height))
        });
        if let Some((x, y, width, height)) = geometry {
            SetWindowPos(
                child,
                Some(HWND_TOP),
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            )?;
            SetWindowTextW(child, button.action.label(button.parent))?;
            let _ = InvalidateRect(Some(child), None, false);
        } else {
            button.hover.set(false);
            button.armed.set(false);
            release_owned_capture(child);
            let _ = ShowWindow(child, SW_HIDE);
        }
        Ok(())
    }
}

unsafe extern "system" fn subclass(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    id: usize,
    data: usize,
) -> LRESULT {
    unsafe {
        let button = &*(data as *const CaptionButton);
        match message {
            WM_NCDESTROY => {
                let _ = RemoveWindowSubclass(hwnd, Some(subclass), id);
                drop(Box::from_raw(data as *mut CaptionButton));
            }
            WM_NCHITTEST | WM_MOUSEMOVE => {
                if message == WM_NCHITTEST {
                    let border = SendMessageW(button.parent, message, Some(wparam), Some(lparam)).0;
                    if (HTLEFT as isize..=HTBOTTOMRIGHT as isize).contains(&border) {
                        return LRESULT(HTTRANSPARENT as isize);
                    }
                }
                let hover = message == WM_NCHITTEST || pointer_inside(hwnd);
                if button.hover.replace(hover) != hover {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                    let flags = if message == WM_NCHITTEST {
                        TME_LEAVE | TME_NONCLIENT
                    } else {
                        TME_LEAVE
                    };
                    let _ = TrackMouseEvent(&mut TRACKMOUSEEVENT {
                        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: flags,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    });
                }
                if message == WM_NCHITTEST {
                    return LRESULT(button.action.hit_test() as isize);
                }
            }
            WM_NCMOUSELEAVE | WM_MOUSELEAVE => {
                button.hover.set(button.armed.get() && pointer_inside(hwnd));
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            WM_CANCELMODE | WM_CAPTURECHANGED => {
                button.hover.set(false);
                button.armed.set(false);
                if message == WM_CANCELMODE {
                    release_owned_capture(hwnd);
                }
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            WM_NCLBUTTONDOWN | WM_LBUTTONDOWN => {
                button.armed.set(true);
                button.hover.set(true);
                let _ = SetCapture(hwnd);
                let _ = InvalidateRect(Some(hwnd), None, false);
                return LRESULT(0);
            }
            WM_NCLBUTTONUP | WM_LBUTTONUP => {
                let clicked = button.armed.replace(false) && pointer_inside(hwnd);
                release_owned_capture(hwnd);
                let _ = InvalidateRect(Some(hwnd), None, false);
                if clicked {
                    button.action.activate(button.parent);
                }
                return LRESULT(0);
            }
            BM_CLICK => {
                button.action.activate(button.parent);
                return LRESULT(0);
            }
            WM_ERASEBKGND => return LRESULT(1),
            WM_PAINT => {
                if let Some(layout) = button.layout.get() {
                    paint::button(
                        hwnd,
                        layout.palette,
                        button.action.glyph(button.parent),
                        button.action == Action::Close,
                        button.hover.get(),
                        button.armed.get() && button.hover.get(),
                    );
                }
                return LRESULT(0);
            }
            WM_PRINT | WM_PRINTCLIENT => {
                if let Some(layout) = button.layout.get() {
                    paint::draw(
                        hwnd,
                        HDC(wparam.0 as _),
                        layout.palette,
                        button.action.glyph(button.parent),
                        button.action == Action::Close,
                        button.hover.get(),
                        button.armed.get() && button.hover.get(),
                    );
                }
                return LRESULT(0);
            }
            _ => {}
        }
        // In particular WM_GETOBJECT stays with the system button proc so
        // Windows accessibility can discover and invoke the caption controls.
        DefSubclassProc(hwnd, message, wparam, lparam)
    }
}

unsafe fn pointer_inside(hwnd: HWND) -> bool {
    unsafe {
        let mut point = POINT::default();
        let mut rect = RECT::default();
        GetCursorPos(&mut point).is_ok()
            && ScreenToClient(hwnd, &mut point).as_bool()
            && GetClientRect(hwnd, &mut rect).is_ok()
            && PtInRect(&rect, point).as_bool()
    }
}

unsafe fn release_owned_capture(hwnd: HWND) {
    unsafe {
        if GetCapture() == hwnd {
            let _ = ReleaseCapture();
        }
    }
}
