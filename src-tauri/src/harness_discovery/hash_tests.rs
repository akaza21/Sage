use std::{fs, os::windows::io::AsRawHandle};

use windows_sys::Win32::Storage::FileSystem::{
    FILE_BASIC_INFO, FileBasicInfo, GetFileInformationByHandleEx, SetFileInformationByHandle,
};

use super::sha256_file;

#[test]
fn same_size_rewrite_with_restored_timestamps_has_a_new_digest() {
    let path = std::env::temp_dir().join(format!("sage-direct-digest-{}", uuid::Uuid::new_v4()));
    fs::write(&path, b"first").expect("write original");
    let original = fs::File::open(&path).expect("open original");
    let mut basic = FILE_BASIC_INFO::default();
    assert_ne!(
        unsafe {
            GetFileInformationByHandleEx(
                original.as_raw_handle(),
                FileBasicInfo,
                (&raw mut basic).cast(),
                u32::try_from(std::mem::size_of::<FILE_BASIC_INFO>()).expect("basic size"),
            )
        },
        0
    );
    drop(original);

    let mut previous = sha256_file(&path).expect("first digest");
    for index in 0..20 {
        let bytes = if index % 2 == 0 { b"other" } else { b"first" };
        fs::write(&path, bytes).expect("rewrite same-sized file");
        let changed = fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("open changed file");
        assert_ne!(
            unsafe {
                SetFileInformationByHandle(
                    changed.as_raw_handle(),
                    FileBasicInfo,
                    (&raw const basic).cast(),
                    u32::try_from(std::mem::size_of::<FILE_BASIC_INFO>()).expect("basic size"),
                )
            },
            0
        );
        drop(changed);
        let current = sha256_file(&path).expect("changed digest");
        assert_ne!(current, previous);
        previous = current;
    }
    fs::remove_file(path).expect("remove scratch file");
}
