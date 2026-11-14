use std::fs;
use std::path::Path;

use super::{StateAuthorityError, TestCodexStateRoot, inspect_test_state};

// WinBase.h flags returned by GetVolumeInformationByHandleW.
const FILE_NAMED_STREAMS: u32 = 0x0004_0000;
pub(super) const FILE_SUPPORTS_HARD_LINKS: u32 = 0x0040_0000;

pub(super) fn assert_named_stream_policy(root: &TestCodexStateRoot, target: &Path) {
    let supported =
        sage_test_support::test_paths::volume_flags(&root.home) & FILE_NAMED_STREAMS != 0;
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
