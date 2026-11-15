use std::os::windows::ffi::OsStringExt;

use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;

use super::*;

const SYSTEM_CMD_EXE: &str = "cmd.exe";

/// Preview reuses the proven Job accounting/termination primitive without
/// inheriting ACP artifact, binding, or batch-launch authority.
pub(crate) struct PreviewJobBoundary(JobBoundary);

impl PreviewJobBoundary {
    pub(crate) fn create() -> Result<Self, LaunchError> {
        Ok(Self(JobBoundary::create(
            u32::try_from(
                sage_core::limits::LIMITS
                    .process
                    .windows_job_active_process_limit,
            )
            .map_err(|_| LaunchError::ContainmentFailure)?,
        )?))
    }

    pub(crate) fn assign_process(&self, process: HANDLE) -> Result<(), LaunchError> {
        // SAFETY: both crate callers pass the raw handle of the preview
        // process their recovery owns, which stays live for the call.
        unsafe { self.0.assign(process) }
    }

    pub(crate) fn terminate_and_wait_zero(&self, deadline: Instant) -> Result<(), LaunchError> {
        self.0.terminate()?;
        self.0.wait_for_zero(
            u32::try_from(
                sage_core::limits::LIMITS
                    .process
                    .windows_job_active_process_limit,
            )
            .map_err(|_| LaunchError::ContainmentFailure)?,
            deadline,
        )
    }

    pub(super) fn verify_exactly_one_member(&self) -> Result<(), LaunchError> {
        match self.0.active_processes(
            u32::try_from(
                sage_core::limits::LIMITS
                    .process
                    .windows_job_active_process_limit,
            )
            .map_err(|_| LaunchError::ContainmentFailure)?,
        )? {
            1 => Ok(()),
            _ => Err(LaunchError::ContainmentFailure),
        }
    }
}

/// Preview-private Windows launch result. This deliberately accepts only the
/// curated `npm run {dev,start}` policy and never exposes ACP's artifact or
/// binding authority.
pub(crate) struct PreviewWindowsProcess {
    process: NativeHandle,
    job: PreviewJobBoundary,
}

pub(crate) enum PreviewLaunchFailure {
    #[allow(
        dead_code,
        reason = "callers only tell a clean failure from a retained one"
    )]
    Clean(LaunchError),
    Retained(PreviewLaunchRecovery),
}

impl From<LaunchError> for PreviewLaunchFailure {
    fn from(error: LaunchError) -> Self {
        Self::Clean(error)
    }
}

/// Exact pre-resume authority retained when bounded convergence has not yet
/// proven zero. Callers must retry or retain this value; dropping it only
/// invokes the Job's kill-on-close backstop and never reports success.
pub(crate) struct PreviewLaunchRecovery {
    pub(super) process: NativeHandle,
    pub(super) thread: Option<NativeHandle>,
    pub(super) stdio: Option<StdioSet>,
    pub(super) job: PreviewJobBoundary,
    pub(super) assigned: bool,
    #[cfg(test)]
    pub(super) retry_zero_fault: Option<LaunchError>,
}

impl PreviewLaunchRecovery {
    pub(crate) fn retry_zero(&mut self) -> Result<(), LaunchError> {
        #[cfg(test)]
        if let Some(error) = self.retry_zero_fault.take() {
            return Err(error);
        }
        suspended::terminate_and_reap_exact(
            &self.job.0,
            self.process.raw(),
            self.assigned,
            sage_core::limits::LIMITS
                .process
                .windows_job_active_process_limit,
            Instant::now()
                + Duration::from_millis(
                    sage_core::limits::PREVIEW_LIMITS.process_termination_deadline_ms,
                ),
        )?;
        self.stdio.take();
        self.thread.take();
        Ok(())
    }

    fn fail(mut self, error: LaunchError) -> PreviewLaunchFailure {
        match self.retry_zero() {
            Ok(()) => PreviewLaunchFailure::Clean(error),
            Err(_) => PreviewLaunchFailure::Retained(self),
        }
    }
}

// Both owned kernel handles are safe to move, but never copied, across the
// lifecycle registry's mutex boundary.
unsafe impl Send for PreviewWindowsProcess {}

impl PreviewWindowsProcess {
    pub(crate) fn launch_curated_npm(
        package_root: &Path,
        script: &'static str,
    ) -> Result<(Self, File, File), PreviewLaunchFailure> {
        if !matches!(script, "dev" | "start") {
            return Err(LaunchError::InvalidArgument.into());
        }
        let package_root = package_root
            .canonicalize()
            .map_err(|_| LaunchError::InvalidAbsolutePath)?;
        let cmd = preview_cmd_exe()?;
        let launcher = preview_npm_cmd()?;
        let expected_launcher = launcher_metadata(&launcher)?;
        let manifest = package_root.join("package.json");
        let expected_manifest = launcher_metadata(&manifest)?;
        let job = PreviewJobBoundary::create()?;
        let application = wide_z(cmd.as_os_str())?;
        // This buffer is intentionally mutable: CreateProcessW may edit it.
        let mut command_line = cmd_batch_launcher_command_line(
            &cmd,
            &launcher,
            &[
                std::ffi::OsString::from("run"),
                std::ffi::OsString::from(script),
            ],
        )?;
        let cwd = wide_z(&cmd_working_directory(&package_root)?)?;
        let suspended =
            match suspended::SuspendedLaunch::create(&application, &mut command_line, &cwd, None) {
                Ok(suspended) => suspended,
                Err(suspended::SuspendedCreateFailure::Clean(error)) => return Err(error.into()),
                Err(suspended::SuspendedCreateFailure::Retained {
                    error,
                    process,
                    stdio,
                }) => {
                    return Err(PreviewLaunchRecovery {
                        process,
                        thread: None,
                        stdio: Some(stdio),
                        job,
                        assigned: false,
                        #[cfg(test)]
                        retry_zero_fault: None,
                    }
                    .fail(error));
                }
            };
        let (process, thread, stdio) = suspended.into_parts();
        let mut recovery = PreviewLaunchRecovery {
            process,
            thread: Some(thread),
            stdio: Some(stdio),
            job,
            assigned: false,
            #[cfg(test)]
            retry_zero_fault: None,
        };
        let outcome = (|| {
            recovery.job.assign_process(recovery.process.raw())?;
            recovery.assigned = true;
            recovery.job.verify_exactly_one_member()?;
            (launcher_metadata(&launcher)? == expected_launcher
                && launcher_metadata(&manifest)? == expected_manifest)
                .then_some(())
                .ok_or(LaunchError::IdentityMismatch)?;
            let thread = recovery
                .thread
                .as_ref()
                .ok_or(LaunchError::ContainmentFailure)?;
            // SAFETY: this is CreateProcessW's original primary-thread handle.
            (unsafe { ResumeThread(thread.raw()) } != u32::MAX)
                .then_some(())
                .ok_or(LaunchError::ContainmentFailure)
        })();
        if let Err(error) = outcome {
            return Err(recovery.fail(error));
        }
        let PreviewLaunchRecovery {
            process,
            thread,
            stdio,
            job,
            ..
        } = recovery;
        drop(thread);
        let (stdin, stdout, stderr) = stdio.ok_or(LaunchError::ContainmentFailure)?.into_parent();
        drop(stdin);
        Ok((Self { process, job }, stdout, stderr))
    }

    pub(crate) fn exited(&self) -> Result<bool, LaunchError> {
        try_observe_exit(self.process.raw())
    }

    pub(crate) fn terminate_and_wait_zero(&self, deadline: Instant) -> Result<(), LaunchError> {
        self.job.terminate_and_wait_zero(deadline)?;
        wait_until(self.process.raw(), deadline)
    }
}

fn preview_cmd_exe() -> Result<PathBuf, LaunchError> {
    let mut directory = vec![0_u16; MAX_WINDOWS_WIDE];
    // SAFETY: directory is writable for the supplied number of UTF-16 units.
    let length = unsafe {
        GetSystemDirectoryW(
            directory.as_mut_ptr(),
            u32::try_from(directory.len()).map_err(|_| LaunchError::InvalidAbsolutePath)?,
        )
    };
    let length = usize::try_from(length).map_err(|_| LaunchError::InvalidAbsolutePath)?;
    if length == 0 || length >= directory.len() {
        return Err(LaunchError::InvalidAbsolutePath);
    }
    let cmd = PathBuf::from(std::ffi::OsString::from_wide(&directory[..length]))
        .join(SYSTEM_CMD_EXE)
        .canonicalize()
        .map_err(|_| LaunchError::InvalidAbsolutePath)?;
    let metadata = cmd
        .metadata()
        .map_err(|_| LaunchError::InvalidAbsolutePath)?;
    metadata
        .is_file()
        .then_some(cmd)
        .ok_or(LaunchError::InvalidAbsolutePath)
}

fn preview_npm_cmd() -> Result<PathBuf, LaunchError> {
    let path = std::env::var_os("PATH").ok_or(LaunchError::InvalidAbsolutePath)?;
    for directory in std::env::split_paths(&path) {
        if !directory.is_absolute() {
            continue;
        }
        let candidate = directory.join("npm.cmd");
        if candidate.is_file() {
            return candidate
                .canonicalize()
                .map_err(|_| LaunchError::InvalidAbsolutePath);
        }
    }
    Err(LaunchError::InvalidAbsolutePath)
}

fn launcher_metadata(path: &Path) -> Result<(u64, std::time::SystemTime), LaunchError> {
    let metadata = path.metadata().map_err(|_| LaunchError::Io)?;
    Ok((
        metadata.len(),
        metadata.modified().map_err(|_| LaunchError::Io)?,
    ))
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use uuid::Uuid;

    use super::*;

    const CHILD_TEST: &str =
        "acp::process::windows::preview::tests::app_preview_system_shell_child";
    const EXPECTED_OUTPUT: &str = "sage-preview-system-shell";

    /// Removes the scratch directory even when an assertion panics.
    struct ScratchDir(std::path::PathBuf);

    impl Drop for ScratchDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn app_preview_system_shell_ignores_comspec() {
        let executable = std::env::current_exe().unwrap();
        let scratch =
            ScratchDir(std::env::temp_dir().join(format!("sage-preview-shell-{}", Uuid::new_v4())));
        let missing = &scratch.0;
        std::fs::create_dir(missing).unwrap();
        for comspec in [
            None,
            Some(missing.join(SYSTEM_CMD_EXE)),
            Some(executable.clone()),
        ] {
            let mut child = Command::new(&executable);
            child.args(["--exact", CHILD_TEST, "--nocapture"]);
            match comspec {
                Some(path) => {
                    child.env("ComSpec", path);
                }
                None => {
                    child.env_remove("ComSpec");
                }
            }
            let output = child.output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stdout)
            );
            assert!(String::from_utf8_lossy(&output.stdout).contains(EXPECTED_OUTPUT));
        }
    }

    #[test]
    fn app_preview_system_shell_child() {
        let cmd = preview_cmd_exe().unwrap();
        let output = Command::new(cmd)
            .args(["/d", "/c", "echo", EXPECTED_OUTPUT])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap().trim(),
            EXPECTED_OUTPUT
        );
        println!("{EXPECTED_OUTPUT}");
    }
}
