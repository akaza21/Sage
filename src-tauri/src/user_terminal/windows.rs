use std::{
    fs::File,
    io::{Read, Write},
    mem::size_of,
    os::windows::{ffi::OsStrExt, io::FromRawHandle},
    path::Path,
    ptr::{null, null_mut},
    time::{Duration, Instant},
};

use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0},
    System::{
        Console::{COORD, ClosePseudoConsole, CreatePseudoConsole, HPCON, ResizePseudoConsole},
        Pipes::CreatePipe,
        Threading::{
            CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessW,
            DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT,
            InitializeProcThreadAttributeList, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE,
            PROCESS_INFORMATION, ResumeThread, STARTUPINFOEXW, TerminateProcess,
            UpdateProcThreadAttribute, WaitForSingleObject,
        },
    },
};

use super::{
    OUTPUT_CHUNK_BYTES, TerminalError, TerminalFactory, TerminalOutputSink, TerminalProcess,
};
use crate::{acp::process::TerminalJobBoundary, project_registry::ProjectRootAuthority};

const DESCENDANT_LIMIT: u32 = 32;
const SETTLE_DEADLINE: Duration = Duration::from_secs(3);

pub(super) struct ConPtyFactory;

impl TerminalFactory for ConPtyFactory {
    fn spawn(
        &self,
        root: &ProjectRootAuthority,
        columns: u16,
        rows: u16,
        output: TerminalOutputSink,
    ) -> Result<Box<dyn TerminalProcess>, TerminalError> {
        root.revalidate().map_err(|_| TerminalError::InvalidScope)?;
        let shell = std::env::var_os("ComSpec").ok_or(TerminalError::Backend)?;
        let shell = Path::new(&shell)
            .canonicalize()
            .map_err(|_| TerminalError::Backend)?;
        if !shell.is_absolute() || !shell.is_file() {
            return Err(TerminalError::Backend);
        }
        let shell_wide = wide(&shell);
        let root_wide = wide(root.as_path());
        if shell_wide.len() > 32_767 || root_wide.len() > 32_767 {
            return Err(TerminalError::Backend);
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
            return Err(TerminalError::Backend);
        }
        let pseudo = PseudoConsole(pseudo);
        drop(input_read);
        drop(output_write);
        let attributes = AttributeList::new(pseudo.0)?;
        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
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
            return Err(TerminalError::Backend);
        }
        let process = Handle(created.hProcess);
        let thread = Handle(created.hThread);
        let job = match TerminalJobBoundary::create(DESCENDANT_LIMIT) {
            Ok(job) => job,
            Err(_) => {
                settle_unassigned(process.0);
                return Err(TerminalError::Backend);
            }
        };
        if job.assign(process.0).is_err() {
            settle_unassigned(process.0);
            return Err(TerminalError::Backend);
        }
        if unsafe { ResumeThread(thread.0) } == u32::MAX {
            let _ = job.terminate();
            let _ = job.wait_for_zero(DESCENDANT_LIMIT, Instant::now() + SETTLE_DEADLINE);
            return Err(TerminalError::Backend);
        }
        let input = input_write.into_file();
        let mut output_file = output_read.into_file();
        let output_thread = std::thread::spawn(move || {
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
        });
        Ok(Box::new(ConPtyProcess {
            job,
            process,
            thread,
            pseudo: Some(pseudo),
            input: Some(input),
            output_thread: Some(output_thread),
        }))
    }
}

struct ConPtyProcess {
    job: TerminalJobBoundary,
    process: Handle,
    thread: Handle,
    pseudo: Option<PseudoConsole>,
    input: Option<File>,
    output_thread: Option<std::thread::JoinHandle<()>>,
}

impl TerminalProcess for ConPtyProcess {
    fn write(&mut self, bytes: &[u8]) -> Result<(), TerminalError> {
        self.input
            .as_mut()
            .ok_or(TerminalError::UnknownSession)?
            .write_all(bytes)
            .map_err(|_| TerminalError::Backend)
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
                (&raw const pseudo).cast(),
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

fn settle_unassigned(process: HANDLE) {
    unsafe { TerminateProcess(process, 1) };
    let _ = unsafe { WaitForSingleObject(process, SETTLE_DEADLINE.as_millis() as u32) }
        == WAIT_OBJECT_0;
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
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
    entries.push(0);
    if entries.len() > 16_384 {
        return Err(TerminalError::Backend);
    }
    Ok(entries)
}
