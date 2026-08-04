fn main() {
    match sage_lib::acp::fixture::run(std::env::args_os().skip(1)) {
        Ok(0) => {}
        Ok(code) => std::process::exit(code),
        Err(_) => std::process::exit(2),
    }
}
