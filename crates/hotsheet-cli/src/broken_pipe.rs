//! Quiet end-of-output when the process reading our stdout/stderr goes away (HS2-4BT3VF).
//!
//! `hotsheet-cli ls | head -1` closes the pipe after one line. Rust ignores `SIGPIPE`, so the
//! next `println!` panics with "failed printing to stdout: Broken pipe (os error 32)". Shell
//! pipelines must never show that, so a closed output pipe is treated as a normal, successful
//! end of output.
//!
//! Only std's print panic is intercepted. An `io::Error` of kind `BrokenPipe` returned from a
//! command is left alone, because it may come from a child process or socket rather than our
//! own stdout, and silencing it there would hide a real failure.
//!
//! We deliberately do **not** restore the default `SIGPIPE` disposition: the same binary runs
//! `serve`, drives AI tool children over pipes, and talks to sockets, where a write to a gone
//! peer must stay an ordinary `EPIPE` error rather than killing the whole process.

/// Exit code used when output ends because the reader closed the pipe.
pub const BROKEN_PIPE_EXIT_CODE: i32 = 0;

/// Whether a panic message is std's "failed printing to stdout/stderr" for a closed pipe.
pub fn is_broken_pipe_print_panic(message: &str) -> bool {
    let printing = message.starts_with("failed printing to stdout")
        || message.starts_with("failed printing to stderr");
    printing && mentions_broken_pipe(message)
}

fn mentions_broken_pipe(message: &str) -> bool {
    if message.contains("Broken pipe") {
        return true;
    }
    #[cfg(unix)]
    {
        message.contains(&format!("(os error {})", libc::EPIPE))
    }
    #[cfg(not(unix))]
    {
        false
    }
}

/// Install a panic hook that turns std's broken-pipe print panic into a quiet exit, delegating
/// every other panic to the previously installed hook unchanged.
pub fn install_broken_pipe_exit() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info.payload();
        let message = payload
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| payload.downcast_ref::<&str>().copied());
        if message.is_some_and(is_broken_pipe_print_panic) {
            std::process::exit(BROKEN_PIPE_EXIT_CODE);
        }
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_std_print_panics_for_a_closed_pipe() {
        assert!(is_broken_pipe_print_panic(
            "failed printing to stdout: Broken pipe (os error 32)"
        ));
        assert!(is_broken_pipe_print_panic(
            "failed printing to stderr: Broken pipe (os error 32)"
        ));
        #[cfg(unix)]
        assert!(is_broken_pipe_print_panic(&format!(
            "failed printing to stdout: localized text (os error {})",
            libc::EPIPE
        )));
    }

    #[test]
    fn leaves_unrelated_panics_alone() {
        assert!(!is_broken_pipe_print_panic(
            "failed printing to stdout: Resource temporarily unavailable (os error 35)"
        ));
        assert!(!is_broken_pipe_print_panic(
            "Broken pipe while talking to child"
        ));
        assert!(!is_broken_pipe_print_panic("index out of bounds"));
    }
}
