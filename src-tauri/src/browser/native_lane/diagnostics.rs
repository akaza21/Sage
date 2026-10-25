//! Native-lane diagnostics (Windows, `native-lane` only): the preflight that
//! runs once the event loop is ready and before any case, the webview probe
//! that attributes a dropped tab, and the logger that surfaces Tauri's and
//! wry's own creation errors (`tauri-runtime-wry` reports a failed child
//! webview only through `log::error!`). Win32 calls are declared here rather
//! than widening the reviewed `windows-sys` feature set.

use std::{
    ffi::c_void,
    path::Path,
    sync::mpsc::{self, RecvTimeoutError},
    time::Duration,
};

use tauri::{AppHandle, Manager, Webview};
use webview2_com::Microsoft::Web::WebView2::Win32::GetAvailableCoreWebView2BrowserVersionString;
use windows_core::{PCWSTR, PWSTR};

const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const UOI_FLAGS: i32 = 1;
const UOI_NAME: i32 = 2;
const WSF_VISIBLE: u32 = 1;
const DESKTOP_SWITCHDESKTOP: u32 = 0x0100;
const WTS_SESSION_INFO_EX: i32 = 25;
const WTS_SESSIONSTATE_LOCK: i32 = 0;

/// The preflight. `Err` is the text after `PREFLIGHT FAIL: `.
pub(super) fn preflight(app: &AppHandle, folders: &[(&str, &Path)]) -> Result<(), String> {
    say(&format!("WebView2 runtime: {}", runtime_version()));
    let session = Session::read();
    say(&session.describe());
    for (name, folder) in folders {
        let entries = std::fs::read_dir(folder).map(Iterator::count);
        say(&format!(
            "{name} user data folder {}: {}",
            folder.display(),
            entries.map_or_else(|error| error.to_string(), |n| format!("{n} entries"))
        ));
    }
    let variables: Vec<String> = std::env::vars()
        .filter(|(name, _)| name.starts_with("WEBVIEW2_"))
        .map(|(name, value)| format!("{name}={value}"))
        .collect();
    say(&format!("WEBVIEW2_* environment: {variables:?}"));
    let main = app
        .get_webview("main")
        .ok_or_else(|| "Tauri holds no main webview".to_owned())
        .and_then(|webview| probe(&webview));
    say(&format!("main webview probe: {}", describe(&main)));
    if !session.interactive() {
        return Err(format!(
            "no interactive window station; WebView2 needs an unlocked interactive desktop ({})",
            session.describe()
        ));
    }
    if session.locked() {
        return Err("workstation is locked; WebView2 cannot start until it is unlocked".into());
    }
    main.map(|_| ())
        .map_err(|error| format!("WebView2 did not start ({error})"))
}

/// Whether the runtime still runs closures on `webview`, and its WebView2
/// browser process. A closure dropped unrun means the runtime holds no such
/// webview (its creation failed or it was closed).
pub(super) fn probe(webview: &Webview) -> Result<u32, String> {
    let (sender, receiver) = mpsc::sync_channel(1);
    webview
        .with_webview(move |platform| {
            let result = (|| -> Result<u32, String> {
                let core = unsafe { platform.controller().CoreWebView2() }
                    .map_err(|error| format!("CoreWebView2 failed: {error}"))?;
                let mut process = 0;
                unsafe { core.BrowserProcessId(&mut process) }
                    .map_err(|error| format!("BrowserProcessId failed: {error}"))?;
                Ok(process)
            })();
            let _ = sender.send(result);
        })
        .map_err(|error| format!("with_webview refused: {error}"))?;
    match receiver.recv_timeout(PROBE_TIMEOUT) {
        Ok(result) => result.map_err(|error| format!("the closure ran, but {error}")),
        Err(RecvTimeoutError::Timeout) => Err(format!(
            "the closure did not run within {} s",
            PROBE_TIMEOUT.as_secs()
        )),
        Err(RecvTimeoutError::Disconnected) => {
            Err("the closure was dropped unrun: the runtime holds no such webview".into())
        }
    }
}

pub(super) fn describe(probe: &Result<u32, String>) -> String {
    match probe {
        Ok(process) => format!("closure ran; CoreWebView2 ok; BrowserProcessId {process}"),
        Err(error) => error.clone(),
    }
}

/// Every webview Tauri still manages, with what the runtime says of each.
pub(super) fn webviews(app: &AppHandle) -> String {
    let mut webviews: Vec<_> = app.webviews().into_iter().collect();
    webviews.sort_by(|left, right| left.0.cmp(&right.0));
    let described: Vec<String> = webviews
        .iter()
        .map(|(label, webview)| format!("{label}: {}", describe(&probe(webview))))
        .collect();
    format!("Tauri webviews [{}]", described.join("; "))
}

fn say(line: &str) {
    println!("[native-lane] preflight: {line}");
}

fn runtime_version() -> String {
    let mut version = PWSTR::null();
    match unsafe { GetAvailableCoreWebView2BrowserVersionString(PCWSTR::null(), &mut version) } {
        Ok(()) if version.is_null() => "not installed".into(),
        Ok(()) => webview2_com::CoTaskMemPWSTR::from(version).to_string(),
        Err(error) => format!("unavailable ({error})"),
    }
}

/// The process's session and desktop, as WebView2 controller creation needs
/// them: a visible window station and an unlocked input desktop.
struct Session {
    id: Result<u32, String>,
    console: u32,
    station: Result<(String, bool), String>,
    input_desktop: Result<(String, bool), String>,
    flags: Result<i32, String>,
}

impl Session {
    fn read() -> Self {
        let mut id = 0;
        let id = if unsafe { process_session(std::process::id(), &mut id) } != 0 {
            Ok(id)
        } else {
            Err(last_error())
        };
        let flags = id.clone().and_then(session_flags);
        Self {
            id,
            console: unsafe { console_session() },
            station: window_station(),
            input_desktop: input_desktop(),
            flags,
        }
    }

    fn interactive(&self) -> bool {
        matches!(self.station, Ok((_, true)))
    }

    /// WTS reports the session locked, or the input desktop is not this
    /// session's `Default` desktop or refuses a switch (the secure desktop
    /// is the input desktop while the workstation is locked).
    fn locked(&self) -> bool {
        self.flags == Ok(WTS_SESSIONSTATE_LOCK)
            || !matches!(&self.input_desktop, Ok((name, true)) if name == "Default")
    }

    fn describe(&self) -> String {
        format!(
            "session {:?} (active console session {}); window station {:?} (name, visible); input desktop {:?} (name, switchable); WTS session flags {:?} (0 locked, 1 unlocked, -1 unknown); locked {}",
            self.id,
            self.console,
            self.station,
            self.input_desktop,
            self.flags,
            self.locked()
        )
    }
}

fn window_station() -> Result<(String, bool), String> {
    let station = unsafe { process_window_station() };
    if station.is_null() {
        return Err(last_error());
    }
    let mut flags = ObjectFlags::default();
    let mut needed = 0;
    let read = unsafe {
        object_information(
            station,
            UOI_FLAGS,
            (&raw mut flags).cast(),
            size_of::<ObjectFlags>() as u32,
            &mut needed,
        )
    };
    if read == 0 {
        return Err(last_error());
    }
    Ok((object_name(station)?, flags.flags & WSF_VISIBLE != 0))
}

/// Opens the input desktop for switching and switches to it: a no-op while
/// it is this session's own desktop, refused while it is locked.
fn input_desktop() -> Result<(String, bool), String> {
    let desktop = unsafe { open_input_desktop(0, 0, DESKTOP_SWITCHDESKTOP) };
    if desktop.is_null() {
        return Err(format!("OpenInputDesktop failed: {}", last_error()));
    }
    let name = object_name(desktop);
    let switchable = unsafe { switch_desktop(desktop) } != 0;
    unsafe { close_desktop(desktop) };
    Ok((name?, switchable))
}

fn object_name(object: *mut c_void) -> Result<String, String> {
    let mut name = [0u16; 256];
    let mut needed = 0;
    let read = unsafe {
        object_information(
            object,
            UOI_NAME,
            name.as_mut_ptr().cast(),
            size_of_val(&name) as u32,
            &mut needed,
        )
    };
    if read == 0 {
        return Err(last_error());
    }
    let end = name
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(name.len());
    Ok(String::from_utf16_lossy(&name[..end]))
}

/// `WTSINFOEXW` level 1's `SessionFlags`.
fn session_flags(session: u32) -> Result<i32, String> {
    let mut buffer = std::ptr::null_mut();
    let mut bytes = 0;
    let queried = unsafe {
        query_session(
            std::ptr::null_mut(),
            session,
            WTS_SESSION_INFO_EX,
            &mut buffer,
            &mut bytes,
        )
    };
    if queried == 0 || buffer.is_null() {
        return Err(format!(
            "WTSQuerySessionInformationW failed: {}",
            last_error()
        ));
    }
    let flags = if (bytes as usize) < size_of::<SessionInfoEx>() {
        Err(format!("WTSSessionInfoEx returned {bytes} bytes"))
    } else {
        let info = unsafe { buffer.cast::<SessionInfoEx>().read_unaligned() };
        if info.level == 1 {
            Ok(info.flags)
        } else {
            Err(format!("WTSSessionInfoEx level {}", info.level))
        }
    };
    unsafe { free_wts(buffer) };
    flags
}

fn last_error() -> String {
    std::io::Error::last_os_error().to_string()
}

/// `USEROBJECTFLAGS`.
#[repr(C)]
#[derive(Default)]
struct ObjectFlags {
    _inherit: i32,
    _reserved: i32,
    flags: u32,
}

/// The head of `WTSINFOEXW`: `Level`, then the level-1 union (8-aligned,
/// as it holds `LARGE_INTEGER`s) whose first fields are `SessionId`,
/// `SessionState` and `SessionFlags`.
#[repr(C)]
struct SessionInfoEx {
    level: u32,
    _padding: u32,
    _session_id: u32,
    _state: i32,
    flags: i32,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    #[link_name = "ProcessIdToSessionId"]
    fn process_session(process: u32, session: *mut u32) -> i32;
    #[link_name = "WTSGetActiveConsoleSessionId"]
    fn console_session() -> u32;
}

#[link(name = "user32")]
unsafe extern "system" {
    #[link_name = "GetProcessWindowStation"]
    fn process_window_station() -> *mut c_void;
    #[link_name = "GetUserObjectInformationW"]
    fn object_information(
        object: *mut c_void,
        index: i32,
        info: *mut c_void,
        length: u32,
        needed: *mut u32,
    ) -> i32;
    #[link_name = "OpenInputDesktop"]
    fn open_input_desktop(flags: u32, inherit: i32, access: u32) -> *mut c_void;
    #[link_name = "SwitchDesktop"]
    fn switch_desktop(desktop: *mut c_void) -> i32;
    #[link_name = "CloseDesktop"]
    fn close_desktop(desktop: *mut c_void) -> i32;
}

#[link(name = "wtsapi32")]
unsafe extern "system" {
    #[link_name = "WTSQuerySessionInformationW"]
    fn query_session(
        server: *mut c_void,
        session: u32,
        class: i32,
        buffer: *mut *mut c_void,
        bytes: *mut u32,
    ) -> i32;
    #[link_name = "WTSFreeMemory"]
    fn free_wts(memory: *mut c_void);
}

/// Prints Tauri's and wry's warnings and errors (a failed child webview is
/// only logged), plus their info lines.
struct LaneLog;

impl log::Log for LaneLog {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
            || metadata.target().starts_with("wry")
            || metadata.target().starts_with("tauri")
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            println!(
                "[native-lane] log {} {}: {}",
                record.level(),
                record.target(),
                record.args()
            );
        }
    }

    fn flush(&self) {}
}

/// Installs `LaneLog` before Tauri builds, so window and webview creation
/// errors reach the lane output.
pub(super) fn install_logger() {
    match log::set_boxed_logger(Box::new(LaneLog)) {
        Ok(()) => log::set_max_level(log::LevelFilter::Info),
        Err(error) => println!("[native-lane] logger not installed: {error}"),
    }
}
