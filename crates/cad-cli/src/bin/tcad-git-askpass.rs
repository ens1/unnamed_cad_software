fn main() {
    if cad_core::git_connections::askpass_main().is_err() {
        // Never print request details or credentials to a process log.
        std::process::exit(1);
    }
}
