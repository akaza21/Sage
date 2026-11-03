use std::ffi::OsString;

use super::*;
use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
use windows_sys::Win32::System::JobObjects::IsProcessInJob;
use windows_sys::Win32::System::Threading::GetExitCodeProcess;

type SuspendedConPty = (
    TerminalJobBoundary,
    PROCESS_INFORMATION,
    bool,
    PseudoConsole,
    Handle,
    Handle,
);

fn launch_suspended_conpty(application: &[u16], command: &[u16]) -> SuspendedConPty {
    launch_suspended_conpty_with(application, command, None)
}

/// With `environment`, the process starts with that block as the terminal's
/// shell does; without it, the process inherits this one's environment.
fn launch_suspended_conpty_with(
    application: &[u16],
    command: &[u16],
    environment: Option<&[u16]>,
) -> SuspendedConPty {
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
    let (flags, block) = match environment {
        Some(block) => (CREATE_UNICODE_ENVIRONMENT, block.as_ptr().cast()),
        None => (0, null()),
    };
    let created = unsafe {
        CreateProcessW(
            application.as_ptr(),
            command.as_mut_ptr(),
            null(),
            null(),
            0,
            CREATE_SUSPENDED | EXTENDED_STARTUPINFO_PRESENT | flags,
            block,
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

#[test]
fn shell_environment_disables_current_directory_command_search() {
    let block = environment_block().unwrap();
    assert!(block.ends_with(&[0, 0]));
    let text = String::from_utf16(&block).unwrap();
    let entries: Vec<&str> = text.split('\0').filter(|entry| !entry.is_empty()).collect();
    let searches: Vec<&str> = entries
        .iter()
        .copied()
        .filter(|entry| {
            entry
                .to_ascii_lowercase()
                .starts_with("nodefaultcurrentdirectoryinexepath=")
        })
        .collect();
    assert_eq!(searches, ["NoDefaultCurrentDirectoryInExePath=1"]);
    assert!(entries.iter().any(|entry| entry.starts_with("ComSpec=")));
}

/// The entries of a block, which must end with two NULs.
fn entries(block: &[u16]) -> Vec<(String, String)> {
    assert!(block.ends_with(&[0, 0]), "{block:?}");
    String::from_utf16(&block[..block.len() - 2])
        .unwrap()
        .split('\0')
        .map(|entry| {
            let (name, value) = entry.split_once('=').unwrap();
            (name.to_owned(), value.to_owned())
        })
        .collect()
}

/// A host that has only `name`, set to `value`.
fn only(name: &'static str, value: OsString) -> impl Fn(&str) -> Option<OsString> {
    move |asked| (asked == name).then(|| value.clone())
}

fn host_value(name: &str) -> String {
    format!(r"C:\Host\{name}")
}

const SEARCH: &str = "NoDefaultCurrentDirectoryInExePath";

/// Every variable the terminal passes, sorted by name ignoring case.
const PASSED_IN_ORDER: [&str; 30] = [
    "ALLUSERSPROFILE",
    "APPDATA",
    "CommonProgramFiles",
    "CommonProgramFiles(x86)",
    "CommonProgramW6432",
    "COMPUTERNAME",
    "ComSpec",
    "HOMEDRIVE",
    "HOMEPATH",
    "LOCALAPPDATA",
    SEARCH,
    "NUMBER_OF_PROCESSORS",
    "OS",
    "PATH",
    "PATHEXT",
    "PROCESSOR_ARCHITECTURE",
    "ProgramData",
    "ProgramFiles",
    "ProgramFiles(x86)",
    "ProgramW6432",
    "PSModulePath",
    "PUBLIC",
    "SystemDrive",
    "SystemRoot",
    "TEMP",
    "TMP",
    "USERDOMAIN",
    "USERNAME",
    "USERPROFILE",
    "windir",
];

#[test]
fn listed_variables_pass_sorted_by_name_and_everything_else_stays_out() {
    // The host has a value for every name, listed or not.
    let host = |name: &str| Some(OsString::from(host_value(name)));
    let received = entries(&environment_block_from(host).unwrap());
    let expected = PASSED_IN_ORDER
        .iter()
        .map(|&name| {
            let value = if name == SEARCH {
                "1".to_owned()
            } else {
                host_value(name)
            };
            (name.to_owned(), value)
        })
        .collect::<Vec<_>>();
    assert_eq!(received, expected);
    for unlisted in [
        "GITHUB_TOKEN",
        "AWS_SECRET_ACCESS_KEY",
        "NODE_OPTIONS",
        "SAGE_TERMINAL_TOKEN",
        "CARGO",
    ] {
        assert!(
            !received
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case(unlisted)),
            "{unlisted} was passed through"
        );
    }
}

#[test]
fn listed_variables_the_host_lacks_are_left_out() {
    let local = r"C:\Users\sage\AppData\Local";
    let block = environment_block_from(only("LOCALAPPDATA", local.into())).unwrap();
    assert_eq!(
        entries(&block),
        [
            ("LOCALAPPDATA".to_owned(), local.to_owned()),
            (SEARCH.to_owned(), "1".to_owned()),
        ]
    );
}

#[test]
fn a_host_value_cannot_override_current_directory_command_search() {
    let host = |name: &str| {
        name.eq_ignore_ascii_case(SEARCH)
            .then(|| OsString::from("0"))
    };
    assert_eq!(
        entries(&environment_block_from(host).unwrap()),
        [(SEARCH.to_owned(), "1".to_owned())]
    );
}

#[test]
fn values_are_copied_as_utf16_without_lossy_conversion() {
    use std::os::windows::ffi::OsStringExt;
    // An unpaired surrogate is not Unicode, so a lossy copy would replace it.
    let value = [u16::from(b'C'), 0xD800, u16::from(b'x')];
    let block = environment_block_from(only("PATH", OsString::from_wide(&value))).unwrap();
    let mut expected = format!("{SEARCH}=1\0PATH=")
        .encode_utf16()
        .collect::<Vec<_>>();
    expected.extend(value);
    expected.extend([0, 0]);
    assert_eq!(block, expected);
}

#[test]
fn long_values_pass_up_to_the_variable_limit_and_the_block_cap_holds() {
    let path = |length: usize| only("PATH", "p".repeat(length).into());
    let received = entries(&environment_block_from(path(12_000)).unwrap());
    assert_eq!(received[1], ("PATH".to_owned(), "p".repeat(12_000)));
    assert!(environment_block_from(path(32_767)).is_ok());
    assert_eq!(
        environment_block_from(path(32_768)),
        Err(TerminalError::Backend)
    );

    // A `PATH` at the limit fits beside every other listed variable.
    let crowded = |name: &str| {
        let length = if name == "PATH" { 32_767 } else { 1_000 };
        Some(OsString::from("v".repeat(length)))
    };
    assert_eq!(
        entries(&environment_block_from(crowded).unwrap()).len(),
        PASSED_IN_ORDER.len()
    );
    // Two variables at the limit are over the block's cap.
    let two = |name: &str| {
        matches!(name, "PATH" | "PSModulePath").then(|| OsString::from("v".repeat(32_767)))
    };
    assert_eq!(environment_block_from(two), Err(TerminalError::Backend));
}

#[test]
fn a_value_holding_nul_fails_closed() {
    let host = only("APPDATA", OsString::from("C:\\App\0Data"));
    assert_eq!(environment_block_from(host), Err(TerminalError::Backend));
}

/// Set in the copy of the test binary that the ConPTY environment test starts.
const HOST_MARKER: &str = "SAGE_CONPTY_TEST_HOST_ENVIRONMENT";
/// The `USERDOMAIN` of that copy. A process with this value and the terminal's
/// `NoDefaultCurrentDirectoryInExePath=1` was started with the terminal's block.
const PROBE_DOMAIN: &str = "SAGE-ENVIRONMENT-PROBE";
const LOCAL_APP_DATA: &str = r"C:\SageProbe\Local";
const RECEIVED_FILE: &str = "received-environment.txt";
const DEADLINE: Duration = Duration::from_secs(30);

fn this_test_command(name: &str) -> Vec<String> {
    let module = module_path!()
        .split_once("::")
        .map_or(module_path!(), |(_, path)| path);
    vec![
        format!("{module}::{name}"),
        "--exact".into(),
        "--test-threads=1".into(),
        "--nocapture".into(),
    ]
}

fn wait_for(output: &TerminalOutputSink, needle: &str) {
    // Subscribing before the first check means no append can be missed.
    let mut changes = output.subscribe();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let text = || String::from_utf8_lossy(&output.snapshot().unwrap().bytes).into_owned();
    let found = runtime.block_on(async {
        tokio::time::timeout(DEADLINE, async {
            while !text().contains(needle) {
                changes.changed().await.unwrap();
            }
        })
        .await
    });
    assert!(found.is_ok(), "no {needle:?} in {:?}", text());
}

/// Reruns this test in a child whose whole environment is a few listed
/// variables, a host value for the command search setting, credentials and a
/// Sage variable. That child starts a copy of the test binary on a ConPTY with
/// the terminal's block, which writes back the environment it received, and
/// then starts a real terminal and reads `%LOCALAPPDATA%` back from `echo`.
#[test]
fn a_process_on_the_conpty_receives_exactly_the_allowlisted_environment() {
    const NAME: &str = "a_process_on_the_conpty_receives_exactly_the_allowlisted_environment";
    let has = |name: &str, value: &str| std::env::var_os(name).is_some_and(|set| set == value);
    if has(SEARCH, "1") && has("USERDOMAIN", PROBE_DOMAIN) {
        let received = std::env::vars_os()
            .map(|(name, value)| format!("{}={}\n", name.display(), value.display()))
            .collect::<String>();
        std::fs::write(std::env::temp_dir().join(RECEIVED_FILE), received).unwrap();
        return;
    }
    if std::env::var_os(HOST_MARKER).is_none() {
        // Setting variables in this multithreaded process would race other
        // tests, so the test reruns itself alone with a controlled environment.
        let directory =
            std::env::temp_dir().join(format!("sage-terminal-env-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let temp = cmd_local_dos_path(&directory.canonicalize().unwrap()).unwrap();
        let system_root = std::env::var_os("SystemRoot").unwrap();
        let host: [(&str, OsString); 14] = [
            ("ComSpec", std::env::var_os("ComSpec").unwrap()),
            ("PATH", Path::new(&system_root).join("system32").into()),
            ("SystemRoot", system_root),
            ("TEMP", temp.into()),
            ("LOCALAPPDATA", LOCAL_APP_DATA.into()),
            ("APPDATA", r"C:\SageProbe\Roaming".into()),
            ("ProgramFiles(x86)", r"C:\SageProbe\Programs".into()),
            ("USERDOMAIN", PROBE_DOMAIN.into()),
            (SEARCH, "0".into()),
            ("GITHUB_TOKEN", "not-for-the-shell".into()),
            ("AWS_SECRET_ACCESS_KEY", "not-for-the-shell".into()),
            ("NODE_OPTIONS", r"--require=C:\not-for-the-shell.js".into()),
            ("SAGE_TERMINAL_TOKEN", "not-for-the-shell".into()),
            (HOST_MARKER, "1".into()),
        ];
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(this_test_command(NAME))
            .env_clear()
            .envs(host)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&child.stdout);
        assert!(
            child.status.success() && stdout.contains("1 passed"),
            "{stdout}\n{}",
            String::from_utf8_lossy(&child.stderr)
        );
        std::fs::remove_dir_all(directory).unwrap();
        return;
    }

    let executable = std::env::current_exe().unwrap();
    let command = std::iter::once(format!("\"{}\"", executable.display()))
        .chain(this_test_command(NAME))
        .collect::<Vec<_>>()
        .join(" ");
    let block = environment_block().unwrap();
    let (job, info, created, pseudo, input, output) =
        launch_suspended_conpty_with(&wide(&executable), &wide_text(&command), Some(&block));
    assert!(created);
    // Draining the console keeps the copy from blocking on its own output.
    let console = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = output.into_file().read_to_end(&mut bytes);
        bytes
    });
    job.assign(info.hProcess).unwrap();
    assert_ne!(unsafe { ResumeThread(info.hThread) }, u32::MAX);
    assert_eq!(
        unsafe { WaitForSingleObject(info.hProcess, DEADLINE.as_millis() as u32) },
        WAIT_OBJECT_0
    );
    let mut exit_code = 1;
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
    let console = console.join().unwrap();
    assert_eq!(exit_code, 0, "{}", String::from_utf8_lossy(&console));
    job.wait_for_zero(DESCENDANT_LIMIT, Instant::now() + SETTLE_DEADLINE)
        .unwrap();
    let temp = std::env::temp_dir();
    let received = std::fs::read_to_string(temp.join(RECEIVED_FILE)).unwrap();
    let received = received
        .lines()
        .map(|line| {
            let (name, value) = line.split_once('=').unwrap();
            (name.to_owned(), value.to_owned())
        })
        .collect::<Vec<_>>();
    let expected = [
        "APPDATA",
        "ComSpec",
        "LOCALAPPDATA",
        SEARCH,
        "PATH",
        "ProgramFiles(x86)",
        "SystemRoot",
        "TEMP",
        "USERDOMAIN",
    ]
    .map(|name| {
        let value = match name {
            SEARCH => "1".to_owned(),
            _ => std::env::var(name).unwrap(),
        };
        (name.to_owned(), value)
    });
    assert_eq!(received, expected);

    let project = temp.join("project");
    std::fs::create_dir(&project).unwrap();
    let root = ProjectRootAuthority::for_test(
        crate::acp::contract::SageProjectId::parse("project-terminal-environment").unwrap(),
        crate::acp::contract::ProjectGeneration::ZERO,
        crate::acp::contract::SageRootId::parse("root-terminal-environment").unwrap(),
        &project.canonicalize().unwrap(),
    )
    .unwrap();
    let output = TerminalOutputSink::new();
    let Ok(mut terminal) = ConPtyFactory.spawn(&root, 80, 24, output.clone()) else {
        panic!("the terminal did not start");
    };
    terminal
        .write(b"echo local=[%LOCALAPPDATA%];token=[%GITHUB_TOKEN%]\r\n")
        .unwrap();
    // cmd.exe leaves the reference to an undefined variable as typed.
    wait_for(
        &output,
        &format!("local=[{LOCAL_APP_DATA}];token=[%GITHUB_TOKEN%]"),
    );
    terminal.terminate().unwrap();
}
