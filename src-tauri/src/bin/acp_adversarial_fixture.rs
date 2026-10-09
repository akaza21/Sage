fn main() {
    if std::env::args()
        .nth(2)
        .is_some_and(|case| sage_lib::acp::fixture::is_native_replay_case(&case))
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
