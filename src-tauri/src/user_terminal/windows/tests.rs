use super::*;
use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
use windows_sys::Win32::System::JobObjects::IsProcessInJob;
use windows_sys::Win32::System::Threading::{TerminateProcess, WaitForSingleObject};

fn launch_with_job(application: &[u16]) -> (TerminalJobBoundary, PROCESS_INFORMATION, bool) {
    let (input_read, _input_write) = pipe().unwrap();
    let (_output_read, output_write) = pipe().unwrap();
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
    let job = TerminalJobBoundary::create(DESCENDANT_LIMIT).unwrap();
    let attributes = AttributeList::new(pseudo.0, job.raw()).unwrap();
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    startup.lpAttributeList = attributes.pointer;
    let mut info = PROCESS_INFORMATION::default();
    let mut command = application.to_vec();
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
    (job, info, created)
}

#[test]
fn shell_is_job_member_before_first_resume() {
    let shell = std::env::var_os("ComSpec").unwrap();
    let application = wide(Path::new(&shell));
    let (job, info, created) = launch_with_job(&application);
    assert!(created);
    let mut member = 0;
    assert_ne!(
        unsafe { IsProcessInJob(info.hProcess, job.raw(), &mut member) },
        0
    );
    let _ = unsafe { TerminateProcess(info.hProcess, 1) };
    let exited = unsafe { WaitForSingleObject(info.hProcess, 3000) } == WAIT_OBJECT_0;
    unsafe {
        CloseHandle(info.hThread);
        CloseHandle(info.hProcess);
    }
    assert!(exited);
    assert_ne!(member, 0);
}

#[test]
fn failed_create_leaves_job_empty() {
    let missing =
        std::env::temp_dir().join(format!("missing-terminal-{}.exe", uuid::Uuid::new_v4()));
    let (job, _, created) = launch_with_job(&wide(&missing));
    assert!(!created);
    job.wait_for_zero(DESCENDANT_LIMIT, Instant::now() + SETTLE_DEADLINE)
        .unwrap();
}
