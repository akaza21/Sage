use std::{
    fs::File,
    io::{Read, Write},
    mem::size_of,
    os::windows::{ffi::OsStrExt, io::FromRawHandle},
    path::{Path, PathBuf},
    ptr::{null, null_mut},
    time::{Duration, Instant},
};

use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::{
        Console::{COORD, ClosePseudoConsole, CreatePseudoConsole, HPCON, ResizePseudoConsole},
        JobObjects::IsProcessInJob,
        Pipes::CreatePipe,
        Threading::{
            CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessW,
            DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT,
            InitializeProcThreadAttributeList, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE,
            PROCESS_INFORMATION, ResumeThread, STARTF_USESTDHANDLES, STARTUPINFOEXW,
            TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject,
        },
    },
};

use super::{
    OUTPUT_CHUNK_BYTES, TerminalError, TerminalFactory, TerminalOutputSink, TerminalProcess,
    TerminalSpawnFailure,
};
use crate::{acp::process::TerminalJobBoundary, project_registry::ProjectRootAuthority};

const DESCENDANT_LIMIT: u32 = 32;
const SETTLE_DEADLINE: Duration = Duration::from_secs(3);
/// Makes `cmd.exe` resolve typed commands from `PATH` only, so a program or
/// script planted in the project folder never shadows a real tool.
const NO_CURRENT_DIRECTORY_COMMAND_SEARCH: (&str, &str) =
    ("NoDefaultCurrentDirectoryInExePath", "1");

pub(super) struct ConPtyFactory;

impl TerminalFactory for ConPtyFactory {
    fn spawn(
        &self,
        root: &ProjectRootAuthority,
        columns: u16,
        rows: u16,
        output: TerminalOutputSink,
    ) -> Result<Box<dyn TerminalProcess>, TerminalSpawnFailure> {
        root.revalidate().map_err(|_| TerminalError::InvalidScope)?;
        let shell = std::env::var_os("ComSpec").ok_or(TerminalError::Backend)?;
        let shell = Path::new(&shell)
            .canonicalize()
            .map_err(|_| TerminalError::Backend)?;
        if !shell.is_absolute() || !shell.is_file() {
            return Err(TerminalError::Backend.into());
        }
        let shell = checked_cmd_local_dos_path(&shell)?;
        let cwd = checked_cmd_local_dos_path(root.as_path())?;
        let shell_wide = wide(&shell);
        let root_wide = wide(&cwd);
        if shell_wide.len() > 32_767 || root_wide.len() > 32_767 {
            return Err(TerminalError::Backend.into());
        }
        let mut command = wide_text(&format!("\"{}\"", shell.to_string_lossy()));
        let environment = environment_block()?;
        let (input_read, input_write) = pipe()?;
        let (output_read, output_write) = pipe()?;
        let mut pseudo = 0;
        let size = COORD {
            X: columns as i16,
            Y: rows as i16,
        };
        if unsafe { CreatePseudoConsole(size, input_read.0, output_write.0, 0, &mut pseudo) } < 0 {
            return Err(TerminalError::Backend.into());
        }
        let pseudo = PseudoConsole(pseudo);
        drop(input_read);
        drop(output_write);
        let job =
            TerminalJobBoundary::create(DESCENDANT_LIMIT).map_err(|_| TerminalError::Backend)?;
        let attributes = AttributeList::new(pseudo.0)?;
        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = INVALID_HANDLE_VALUE;
        startup.StartupInfo.hStdOutput = INVALID_HANDLE_VALUE;
        startup.StartupInfo.hStdError = INVALID_HANDLE_VALUE;
        startup.lpAttributeList = attributes.pointer;
        let mut created = PROCESS_INFORMATION::default();
        let launched = unsafe {
            CreateProcessW(
                shell_wide.as_ptr(),
                command.as_mut_ptr(),
                null(),
                null(),
                0,
                CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT,
                environment.as_ptr().cast(),
                root_wide.as_ptr(),
                (&raw const startup.StartupInfo),
                &mut created,
            )
        };
        if launched == 0 {
            return Err(TerminalError::Backend.into());
        }
        let process = Handle(created.hProcess);
        let thread = Handle(created.hThread);
        let mut running = ConPtyProcess {
            shell_path: shell.to_string_lossy().into_owned(),
            job,
            process,
            thread,
            assigned: false,
            pseudo: Some(pseudo),
            input: Some(input_write.into_file()),
            output_thread: None,
        };
        if contain_and_resume(&mut running, |job, process| {
            job.assign(process).map_err(|_| TerminalError::Backend)
        })
        .is_err()
        {
            return match running.abort_launch() {
                Ok(()) => Err(TerminalError::Backend.into()),
                Err(_) => Err(TerminalSpawnFailure::Retained(Box::new(running))),
            };
        }
        let mut output_file = output_read.into_file();
        running.output_thread = Some(std::thread::spawn(move || {
            let mut chunk = [0_u8; OUTPUT_CHUNK_BYTES];
            loop {
                match output_file.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => {
                        if output.append(&chunk[..count]).is_err() {
                            break;
                        }
                    }
                }
            }
        }));
        Ok(Box::new(running))
    }
}

struct ConPtyProcess {
    shell_path: String,
    job: TerminalJobBoundary,
    process: Handle,
    thread: Handle,
    assigned: bool,
    pseudo: Option<PseudoConsole>,
    input: Option<File>,
    output_thread: Option<std::thread::JoinHandle<()>>,
}

fn contain_and_resume(
    running: &mut ConPtyProcess,
    assign: impl FnOnce(&TerminalJobBoundary, HANDLE) -> Result<(), TerminalError>,
) -> Result<(), TerminalError> {
    assign(&running.job, running.process.0)?;
    let mut member = 0;
    if unsafe { IsProcessInJob(running.process.0, running.job.raw(), &mut member) } == 0
        || member == 0
    {
        return Err(TerminalError::Backend);
    }
    running.assigned = true;
    if unsafe { ResumeThread(running.thread.0) } == u32::MAX {
        return Err(TerminalError::Backend);
    }
    Ok(())
}

impl ConPtyProcess {
    fn exited(&self) -> Result<bool, TerminalError> {
        match unsafe { WaitForSingleObject(self.process.0, 0) } {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            _ => Err(TerminalError::Backend),
        }
    }

    fn abort_launch(&mut self) -> Result<(), TerminalError> {
        let _ = unsafe { TerminateProcess(self.process.0, 1) };
        if unsafe { WaitForSingleObject(self.process.0, SETTLE_DEADLINE.as_millis() as u32) }
            != WAIT_OBJECT_0
        {
            return Err(TerminalError::Backend);
        }
        if self.assigned {
            let _ = self.job.terminate();
        }
        self.job
            .wait_for_zero(DESCENDANT_LIMIT, Instant::now() + SETTLE_DEADLINE)
            .map_err(|_| TerminalError::Backend)?;
        self.input.take();
        self.pseudo.take();
        Ok(())
    }
}

impl TerminalProcess for ConPtyProcess {
    fn shell_path(&self) -> &str {
        &self.shell_path
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), TerminalError> {
        if self.exited()? {
            return Err(TerminalError::Ended);
        }
        let result = self
            .input
            .as_mut()
            .ok_or(TerminalError::UnknownSession)?
            .write_all(bytes);
        if result.is_err() {
            return Err(if self.exited()? {
                TerminalError::Ended
            } else {
                TerminalError::Backend
            });
        }
        Ok(())
    }

    fn resize(&mut self, columns: u16, rows: u16) -> Result<(), TerminalError> {
        let pseudo = self.pseudo.as_ref().ok_or(TerminalError::UnknownSession)?;
        let size = COORD {
            X: columns as i16,
            Y: rows as i16,
        };
        if unsafe { ResizePseudoConsole(pseudo.0, size) } < 0 {
            Err(TerminalError::Backend)
        } else {
            Ok(())
        }
    }

    fn terminate(&mut self) -> Result<(), TerminalError> {
        if !self.assigned {
            return self.abort_launch();
        }
        let _ = (&self.process, &self.thread);
        self.job.terminate().map_err(|_| TerminalError::Backend)?;
        self.job
            .wait_for_zero(DESCENDANT_LIMIT, Instant::now() + SETTLE_DEADLINE)
            .map_err(|_| TerminalError::Backend)?;
        self.input.take();
        self.pseudo.take();
        if let Some(thread) = self.output_thread.take() {
            thread.join().map_err(|_| TerminalError::Backend)?;
        }
        Ok(())
    }
}

impl Drop for ConPtyProcess {
    fn drop(&mut self) {
        let _ = self.terminate();
    }
}

struct Handle(HANDLE);

unsafe impl Send for Handle {}

impl Handle {
    fn into_file(mut self) -> File {
        let raw = self.0;
        self.0 = null_mut();
        unsafe { File::from_raw_handle(raw) }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            unsafe { CloseHandle(self.0) };
        }
    }
}

struct PseudoConsole(HPCON);

unsafe impl Send for PseudoConsole {}

impl Drop for PseudoConsole {
    fn drop(&mut self) {
        if self.0 != 0 {
            unsafe { ClosePseudoConsole(self.0) };
        }
    }
}

struct AttributeList {
    _storage: Vec<usize>,
    pointer: *mut core::ffi::c_void,
}

impl AttributeList {
    fn new(pseudo: HPCON) -> Result<Self, TerminalError> {
        let mut bytes = 0_usize;
        unsafe { InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut bytes) };
        if bytes == 0 {
            return Err(TerminalError::Backend);
        }
        let mut storage = vec![0_usize; bytes.div_ceil(size_of::<usize>())];
        let pointer = storage.as_mut_ptr().cast();
        if unsafe { InitializeProcThreadAttributeList(pointer, 1, 0, &mut bytes) } == 0 {
            return Err(TerminalError::Backend);
        }
        if unsafe {
            UpdateProcThreadAttribute(
                pointer,
                0,
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                pseudo as *const core::ffi::c_void,
                size_of::<HPCON>(),
                null_mut(),
                null(),
            )
        } == 0
        {
            unsafe { DeleteProcThreadAttributeList(pointer) };
            return Err(TerminalError::Backend);
        }
        Ok(Self {
            _storage: storage,
            pointer,
        })
    }
}

impl Drop for AttributeList {
    fn drop(&mut self) {
        unsafe { DeleteProcThreadAttributeList(self.pointer) };
    }
}

fn pipe() -> Result<(Handle, Handle), TerminalError> {
    let mut read = null_mut();
    let mut write = null_mut();
    if unsafe { CreatePipe(&mut read, &mut write, null(), 0) } == 0 {
        return Err(TerminalError::Backend);
    }
    Ok((Handle(read), Handle(write)))
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn cmd_local_dos_path(path: &Path) -> Result<PathBuf, TerminalError> {
    let path = path.to_str().ok_or(TerminalError::Backend)?;
    let path = path.strip_prefix(r"\\?\").unwrap_or(path);
    let bytes = path.as_bytes();
    if path.starts_with(r"\\")
        || bytes.len() < 3
        || !bytes[0].is_ascii_alphabetic()
        || bytes[1] != b':'
        || bytes[2] != b'\\'
    {
        return Err(TerminalError::Backend);
    }
    Ok(PathBuf::from(path))
}

fn checked_cmd_local_dos_path(path: &Path) -> Result<PathBuf, TerminalError> {
    let dos_path = cmd_local_dos_path(path)?;
    if dos_path
        .canonicalize()
        .map_err(|_| TerminalError::Backend)?
        != path
    {
        return Err(TerminalError::Backend);
    }
    Ok(dos_path)
}
fn wide_text(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn environment_block() -> Result<Vec<u16>, TerminalError> {
    let mut entries = Vec::new();
    for name in [
        "ComSpec",
        "SystemRoot",
        "TEMP",
        "TMP",
        "PATH",
        "PATHEXT",
        "USERPROFILE",
    ] {
        if let Some(value) = std::env::var_os(name) {
            let value = value.to_string_lossy();
            if value.contains('\0') || value.len() > 8_192 {
                return Err(TerminalError::Backend);
            }
            entries.extend(format!("{name}={value}").encode_utf16());
            entries.push(0);
        }
    }
    let (name, value) = NO_CURRENT_DIRECTORY_COMMAND_SEARCH;
    entries.extend(format!("{name}={value}").encode_utf16());
    entries.push(0);
    entries.push(0);
    if entries.len() > 16_384 {
        return Err(TerminalError::Backend);
    }
    Ok(entries)
}

#[cfg(test)]
mod tests;
