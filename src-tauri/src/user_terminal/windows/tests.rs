use super::*;
use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
use windows_sys::Win32::System::JobObjects::IsProcessInJob;
use windows_sys::Win32::System::Threading::GetExitCodeProcess;

fn launch_suspended_conpty(
    application: &[u16],
    command: &[u16],
) -> (
    TerminalJobBoundary,
    PROCESS_INFORMATION,
    bool,
    PseudoConsole,
    Handle,
    Handle,
) {
    let (input_read, input_write) = pipe().unwrap();
    let (output_read, output_write) = pipe().unwrap();
    let mut pseudo = 0;
    assert!(
        unsafe {
            CreatePseudoConsole(
                COORD { X: 80, Y: 24 },
                input_read.0,
                output_write.0,
                0,
                &mut pseudo,
            )
        } >= 0
    );
    let pseudo = PseudoConsole(pseudo);
    drop(input_read);
    drop(output_write);
    let job = TerminalJobBoundary::create(DESCENDANT_LIMIT).unwrap();
    let attributes = AttributeList::new(pseudo.0).unwrap();
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = INVALID_HANDLE_VALUE;
    startup.StartupInfo.hStdOutput = INVALID_HANDLE_VALUE;
    startup.StartupInfo.hStdError = INVALID_HANDLE_VALUE;
    startup.lpAttributeList = attributes.pointer;
    let mut info = PROCESS_INFORMATION::default();
    let mut command = command.to_vec();
    let created = unsafe {
        CreateProcessW(
            application.as_ptr(),
            command.as_mut_ptr(),
            null(),
            null(),
            0,
            CREATE_SUSPENDED | EXTENDED_STARTUPINFO_PRESENT,
            null(),
            null(),
            (&raw const startup.StartupInfo),
            &mut info,
        )
    } != 0;
    (job, info, created, pseudo, input_write, output_read)
}

fn shell() -> (PathBuf, Vec<u16>) {
    let shell = PathBuf::from(std::env::var_os("ComSpec").unwrap());
    let application = wide(&shell);
    (shell, application)
}

fn member(job: &TerminalJobBoundary, process: HANDLE) -> bool {
    let mut member = 0;
    assert_ne!(
        unsafe { IsProcessInJob(process, job.raw(), &mut member) },
        0
    );
    member != 0
}

#[test]
fn shell_is_job_member_before_first_resume() {
    let (_, application) = shell();
    let (job, info, created, _pseudo, _input, _output) =
        launch_suspended_conpty(&application, &application);
    assert!(created);
    assert!(!member(&job, info.hProcess));
    job.assign(info.hProcess).unwrap();
    assert!(member(&job, info.hProcess));
    let _ = unsafe { TerminateProcess(info.hProcess, 1) };
    assert_eq!(
        unsafe { WaitForSingleObject(info.hProcess, 3_000) },
        WAIT_OBJECT_0
    );
    job.wait_for_zero(DESCENDANT_LIMIT, Instant::now() + SETTLE_DEADLINE)
        .unwrap();
    unsafe {
        CloseHandle(info.hThread);
        CloseHandle(info.hProcess);
    }
}

#[test]
fn failed_create_leaves_job_empty() {
    let missing =
        std::env::temp_dir().join(format!("missing-terminal-{}.exe", uuid::Uuid::new_v4()));
    let application = wide(&missing);
    let (job, _, created, _pseudo, _input, _output) =
        launch_suspended_conpty(&application, &application);
    assert!(!created);
    job.wait_for_zero(DESCENDANT_LIMIT, Instant::now() + SETTLE_DEADLINE)
        .unwrap();
}

#[test]
fn contained_conpty_echo_and_zero_members_after_close() {
    let (shell, application) = shell();
    let command = wide_text(&format!("\"{}\" /d /c echo CONPTY_JOB_OK", shell.display()));
    let (job, info, created, pseudo, input, output) =
        launch_suspended_conpty(&application, &command);
    assert!(created);
    job.assign(info.hProcess).unwrap();
    assert!(member(&job, info.hProcess));
    assert_ne!(unsafe { ResumeThread(info.hThread) }, u32::MAX);
    assert_eq!(
        unsafe { WaitForSingleObject(info.hProcess, 3_000) },
        WAIT_OBJECT_0
    );
    let mut exit_code = 0;
    assert_ne!(
        unsafe { GetExitCodeProcess(info.hProcess, &mut exit_code) },
        0
    );
    unsafe {
        CloseHandle(info.hThread);
        CloseHandle(info.hProcess);
    }
    drop(input);
    drop(pseudo);
    let mut bytes = Vec::new();
    output.into_file().read_to_end(&mut bytes).unwrap();
    assert_eq!(exit_code, 0);
    assert!(String::from_utf8_lossy(&bytes).contains("CONPTY_JOB_OK"));
    job.wait_for_zero(DESCENDANT_LIMIT, Instant::now() + SETTLE_DEADLINE)
        .unwrap();
}

#[test]
fn writing_after_shell_exit_reports_session_ended() {
    let (shell, application) = shell();
    let command = wide_text(&format!("\"{}\" /d /c exit 0", shell.display()));
    let (job, info, created, pseudo, input, output) =
        launch_suspended_conpty(&application, &command);
    assert!(created);
    job.assign(info.hProcess).unwrap();
    assert!(member(&job, info.hProcess));
    assert_ne!(unsafe { ResumeThread(info.hThread) }, u32::MAX);
    assert_eq!(
        unsafe { WaitForSingleObject(info.hProcess, 3_000) },
        WAIT_OBJECT_0
    );
    drop(output);
    let mut running = ConPtyProcess {
        shell_path: shell.to_string_lossy().into_owned(),
        job,
        process: Handle(info.hProcess),
        thread: Handle(info.hThread),
        assigned: true,
        pseudo: Some(pseudo),
        input: Some(input.into_file()),
        output_thread: None,
    };
    assert_eq!(running.write(b"late input"), Err(TerminalError::Ended));
}

#[test]
fn assignment_failure_never_resumes_shell_and_settles_exact_process() {
    let (_, application) = shell();
    let (job, info, created, pseudo, input, output) =
        launch_suspended_conpty(&application, &application);
    assert!(created);
    drop(output);
    let mut running = ConPtyProcess {
        shell_path: "test shell".into(),
        job,
        process: Handle(info.hProcess),
        thread: Handle(info.hThread),
        assigned: false,
        pseudo: Some(pseudo),
        input: Some(input.into_file()),
        output_thread: None,
    };
    assert!(contain_and_resume(&mut running, |_, _| Err(TerminalError::Backend)).is_err());
    assert!(!member(&running.job, running.process.0));
    running.abort_launch().unwrap();
    assert_eq!(
        unsafe { WaitForSingleObject(running.process.0, 0) },
        WAIT_OBJECT_0
    );
    running
        .job
        .wait_for_zero(DESCENDANT_LIMIT, Instant::now() + SETTLE_DEADLINE)
        .unwrap();
}

#[test]
fn cmd_launch_paths_strip_local_verbatim_prefix_and_reject_unc() {
    assert_eq!(
        cmd_local_dos_path(Path::new(r"\\?\C:\Windows\System32\cmd.exe")),
        Ok(PathBuf::from(r"C:\Windows\System32\cmd.exe"))
    );
    assert_eq!(
        cmd_local_dos_path(Path::new(r"\\?\C:\Sage\project")),
        Ok(PathBuf::from(r"C:\Sage\project"))
    );
    for path in [
        r"\\?\UNC\server\share\project",
        r"\\server\share\project",
        r"\\.\C:\device",
    ] {
        assert_eq!(
            cmd_local_dos_path(Path::new(path)),
            Err(TerminalError::Backend)
        );
    }
}

#[test]
fn cmd_launch_path_must_recanonicalize_to_same_authority() {
    let canonical = std::env::temp_dir().canonicalize().unwrap();
    let alias = PathBuf::from(format!(r"{}\..", canonical.display()));
    assert!(alias.to_string_lossy().ends_with(r"\.."));
    assert!(checked_cmd_local_dos_path(&canonical).is_ok());
    assert_eq!(
        checked_cmd_local_dos_path(&alias),
        Err(TerminalError::Backend)
    );
}
