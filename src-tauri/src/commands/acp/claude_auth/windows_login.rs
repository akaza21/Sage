use std::{ffi::OsString, mem::size_of, path::Path, ptr::null};

use windows_sys::Win32::{
    Foundation::CloseHandle,
    System::Threading::{
        CREATE_NEW_CONSOLE, CREATE_UNICODE_ENVIRONMENT, CreateProcessW, PROCESS_INFORMATION,
        STARTUPINFOW,
    },
};

use super::ClaudeEnvironment;

pub(super) fn launch_login(
    path: &Path,
    environment: &ClaudeEnvironment,
    cwd: &Path,
) -> Result<(), &'static str> {
    let application = crate::acp::process::windows_wide_z(path.as_os_str())
        .map_err(|_| "Claude Code sign-in could not start.")?;
    let mut command_line = crate::acp::process::windows_command_line(
        path,
        &[OsString::from("auth"), OsString::from("login")],
    )
    .map_err(|_| "Claude Code sign-in could not start.")?;
    let environment = crate::acp::process::windows_environment_block(environment)
        .map_err(|_| "Claude Code sign-in could not start.")?;
    let cwd = crate::acp::process::windows_wide_z(cwd.as_os_str())
        .map_err(|_| "Claude Code sign-in could not start.")?;
    let startup = STARTUPINFOW {
        cb: u32::try_from(size_of::<STARTUPINFOW>())
            .map_err(|_| "Claude Code sign-in could not start.")?,
        ..STARTUPINFOW::default()
    };
    let mut process = PROCESS_INFORMATION::default();
    if unsafe {
        CreateProcessW(
            application.as_ptr(),
            command_line.as_mut_ptr(),
            null(),
            null(),
            0,
            CREATE_NEW_CONSOLE | CREATE_UNICODE_ENVIRONMENT,
            environment.as_ptr().cast(),
            cwd.as_ptr(),
            &startup,
            &mut process,
        )
    } == 0
    {
        return Err("Claude Code sign-in could not start.");
    }
    unsafe {
        CloseHandle(process.hThread);
        CloseHandle(process.hProcess);
    }
    Ok(())
}
