//! The `cuno` binary: collects what only the process has, hands it to
//! `cuno_cli::run`, and ends the process with the code that returns. Nothing
//! else goes in `src/` (`docs/en/specs/architecture.md`, "What goes in `src/`
//! and `cuno-cli`").

use std::io::IsTerminal;

use anstream::AutoStream;

fn main() {
    let code = cuno_cli::run(cuno_cli::Process {
        args: std::env::args_os().collect(),
        env: cuno_core::Env::from_process(),
        cwd: std::env::current_dir().map_err(|e| e.to_string()),
        host: cuno_cli::HostOs::current(),
        temp: std::env::temp_dir(),
        stdout_is_terminal: std::io::stdout().is_terminal(),
        stderr_is_terminal: std::io::stderr().is_terminal(),
        // cuno decides whether to color and writes escape codes itself. On a
        // Windows console these turn on its support for them, or are turned
        // into console calls where it has none; through a pipe they pass
        // unchanged.
        stdout: Box::new(AutoStream::always(std::io::stdout())),
        stderr: Box::new(AutoStream::always(std::io::stderr())),
    });
    if code != 0 {
        std::process::exit(code);
    }
    // A panic anywhere exits with Rust's own code, 101, the spec's code for an
    // internal error. This holds while `panic = "abort"` is not set in any
    // profile; tests/cli.rs, `a_panic_exits_101`, guards it.
}
