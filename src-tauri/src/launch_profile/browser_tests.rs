use std::{
    fs,
    sync::atomic::{AtomicUsize, Ordering},
};

use super::{PROFILE_MARKER_BYTES, PROFILE_MARKER_NAME, resolve_launch_profile};

static NEXT_ROOT: AtomicUsize = AtomicUsize::new(1);

#[test]
fn disposable_profile_accepts_browser_directory_on_next_launch() {
    let root = std::env::temp_dir().join(format!(
        "sage-browser-relaunch-{}-{}",
        std::process::id(),
        NEXT_ROOT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root).unwrap();
    fs::write(root.join(PROFILE_MARKER_NAME), PROFILE_MARKER_BYTES).unwrap();
    let normal_durable = root.with_extension("durable");
    let normal_webview = root.with_extension("webview");
    resolve_launch_profile(
        Some(root.as_os_str()),
        None,
        normal_durable.clone(),
        normal_webview.clone(),
    )
    .unwrap();
    fs::create_dir(root.join("browser")).unwrap();
    assert!(
        resolve_launch_profile(Some(root.as_os_str()), None, normal_durable, normal_webview,)
            .is_ok()
    );
    fs::remove_dir_all(root).unwrap();
}
