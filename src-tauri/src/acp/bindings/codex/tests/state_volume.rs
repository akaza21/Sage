use std::fs::{self, OpenOptions};
use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
use std::path::Path;

use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_BACKUP_SEMANTICS, GetVolumeInformationByHandleW,
};

use super::{StateAuthorityError, TestCodexStateRoot, inspect_test_state};

// WinBase.h flags returned by GetVolumeInformationByHandleW.
const FILE_NAMED_STREAMS: u32 = 0x0004_0000;
pub(super) const FILE_SUPPORTS_HARD_LINKS: u32 = 0x0040_0000;

pub(super) fn capabilities(directory: &Path) -> u32 {
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(directory)
        .unwrap();
    let mut flags = 0;
    assert_ne!(
        unsafe {
            GetVolumeInformationByHandleW(
                directory.as_raw_handle().cast(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut flags,
                std::ptr::null_mut(),
                0,
            )
        },
        0,
        "inspect fixture volume capabilities"
    );
    flags
}

pub(super) fn assert_named_stream_policy(root: &TestCodexStateRoot, target: &Path) {
    let supported = capabilities(&root.home) & FILE_NAMED_STREAMS != 0;
    let mut stream = target.as_os_str().to_os_string();
    stream.push(":secret");
    let write = fs::write(stream, b"hidden");
    if supported {
        write.expect("create supported alternate stream");
        assert!(matches!(
            inspect_test_state(root),
            Err(StateAuthorityError::Stream)
        ));
    } else {
        assert!(write.is_err(), "a streamless volume must refuse an ADS");
        // Absence of the capability proves that no hidden stream can exist.
        inspect_test_state(root).expect("bounded state on a streamless volume");
    }
}
