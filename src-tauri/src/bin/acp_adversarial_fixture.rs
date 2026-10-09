fn main() {
    if std::env::args_os()
        .nth(2)
        .as_deref()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(sage_lib::acp::fixture::is_native_replay_case)
    {
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(120));
            std::process::exit(3);
        });
    }
    match sage_lib::acp::fixture::run(std::env::args_os().skip(1)) {
        Ok(0) => {}
        Ok(code) => std::process::exit(code),
        Err(_) => std::process::exit(2),
    }
}
