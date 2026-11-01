use std::fs::File;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;

use windows_sys::Win32::Foundation::{
    ERROR_HANDLE_EOF, ERROR_INVALID_FUNCTION, ERROR_INVALID_PARAMETER, ERROR_NOT_SUPPORTED,
    GetLastError, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_NAMED_STREAMS, FILE_STREAM_INFO, FileStreamInfo, FindClose, FindFirstStreamW,
    FindNextStreamW, FindStreamInfoStandard, GetFileInformationByHandleEx,
    GetVolumeInformationByHandleW, WIN32_FIND_STREAM_DATA,
};

use super::StateAuthorityError;

const STREAM_INFO_BYTES_MAX: usize = 64 * 1024;

pub(super) fn reject_named_streams(file: &File) -> Result<(), StateAuthorityError> {
    let mut buffer = vec![0_u64; STREAM_INFO_BYTES_MAX / std::mem::size_of::<u64>()];
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle() as HANDLE,
            FileStreamInfo,
            buffer.as_mut_ptr().cast(),
            u32::try_from(STREAM_INFO_BYTES_MAX).map_err(|_| StateAuthorityError::Stream)?,
        )
    } == 0
    {
        return unsupported_stream_query(file, unsafe { GetLastError() });
    }
    let entry = unsafe { &*buffer.as_ptr().cast::<FILE_STREAM_INFO>() };
    let name_bytes =
        usize::try_from(entry.StreamNameLength).map_err(|_| StateAuthorityError::Stream)?;
    let name_start = std::mem::offset_of!(FILE_STREAM_INFO, StreamName);
    let name_end = name_start
        .checked_add(name_bytes)
        .filter(|end| *end <= STREAM_INFO_BYTES_MAX && name_bytes.is_multiple_of(2))
        .ok_or(StateAuthorityError::Stream)?;
    let name = unsafe {
        std::slice::from_raw_parts(
            buffer.as_ptr().cast::<u8>().add(name_start).cast::<u16>(),
            (name_end - name_start) / 2,
        )
    };
    let name = String::from_utf16(name).map_err(|_| StateAuthorityError::Stream)?;
    if entry.NextEntryOffset != 0 || name != "::$DATA" {
        return Err(StateAuthorityError::Stream);
    }
    Ok(())
}

pub(super) fn reject_directory_named_streams(
    directory: &File,
    path: &Path,
) -> Result<(), StateAuthorityError> {
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut entry = WIN32_FIND_STREAM_DATA::default();
    let find = unsafe {
        FindFirstStreamW(
            path.as_ptr(),
            FindStreamInfoStandard,
            (&mut entry as *mut WIN32_FIND_STREAM_DATA).cast(),
            0,
        )
    };
    if find == INVALID_HANDLE_VALUE {
        let error = unsafe { GetLastError() };
        return if error == ERROR_HANDLE_EOF {
            Ok(())
        } else {
            unsupported_stream_query(directory, error)
        };
    }

    let result = inspect_directory_streams(find, &mut entry);
    if unsafe { FindClose(find) } == 0 {
        return Err(StateAuthorityError::Stream);
    }
    result
}

// Unsupported enumeration is not evidence of absence. Require the volume of
// the retained handle to prove that named streams cannot exist at all.
fn unsupported_stream_query(file: &File, error: u32) -> Result<(), StateAuthorityError> {
    if !matches!(
        error,
        ERROR_INVALID_FUNCTION | ERROR_INVALID_PARAMETER | ERROR_NOT_SUPPORTED
    ) {
        return Err(StateAuthorityError::Stream);
    }
    let mut flags = 0;
    if unsafe {
        GetVolumeInformationByHandleW(
            file.as_raw_handle() as HANDLE,
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut flags,
            std::ptr::null_mut(),
            0,
        )
    } == 0
    {
        return Err(StateAuthorityError::Stream);
    }
    require_streamless_volume(flags)
}

fn require_streamless_volume(flags: u32) -> Result<(), StateAuthorityError> {
    if flags & FILE_NAMED_STREAMS == 0 {
        Ok(())
    } else {
        Err(StateAuthorityError::Stream)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_enumeration_requires_proof_that_the_volume_has_no_named_streams() {
        assert_eq!(require_streamless_volume(0), Ok(()));
        assert_eq!(
            require_streamless_volume(FILE_NAMED_STREAMS),
            Err(StateAuthorityError::Stream)
        );
        let file = std::fs::File::open(std::env::current_exe().unwrap()).unwrap();
        // An access or I/O failure must never fall back to a capability claim.
        assert_eq!(
            unsupported_stream_query(&file, windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED),
            Err(StateAuthorityError::Stream)
        );
    }
}

fn inspect_directory_streams(
    find: HANDLE,
    entry: &mut WIN32_FIND_STREAM_DATA,
) -> Result<(), StateAuthorityError> {
    if stream_name(entry)? != "::$DATA" {
        return Err(StateAuthorityError::Stream);
    }
    if unsafe { FindNextStreamW(find, (entry as *mut WIN32_FIND_STREAM_DATA).cast()) } != 0
        || unsafe { GetLastError() } != ERROR_HANDLE_EOF
    {
        return Err(StateAuthorityError::Stream);
    }
    Ok(())
}

fn stream_name(entry: &WIN32_FIND_STREAM_DATA) -> Result<String, StateAuthorityError> {
    let end = entry
        .cStreamName
        .iter()
        .position(|character| *character == 0)
        .ok_or(StateAuthorityError::Stream)?;
    String::from_utf16(&entry.cStreamName[..end]).map_err(|_| StateAuthorityError::Stream)
}
