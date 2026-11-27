//! Classifying errors into the exit codes of `docs/en/specs/commands.md`,
//! "Exit codes".

/// The kind of failure, which decides the exit code. Every error a crate
/// returns says which kind it is, so the binary never has to know the
/// crate's error types to choose an exit code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitClass {
    /// A problem with the user's code or build artifacts (planned: a compile
    /// error, a failing test, unformatted code under `fmt --check`, a problem
    /// found by `doctor`).
    Failure,
    /// Incorrect usage of the command (an unknown option, a missing argument).
    Usage,
    /// A configuration or environment problem: the setup cuno runs in is
    /// wrong, not the user's code (an error in `Cuno.toml`, no compiler found,
    /// a compiler version that does not satisfy a constraint).
    Setup,
    /// A bug in cuno itself.
    Internal,
}

impl ExitClass {
    /// The process exit code for this class.
    pub fn code(self) -> i32 {
        match self {
            ExitClass::Failure => 1,
            ExitClass::Usage => 2,
            ExitClass::Setup => 3,
            // The same value as a Rust panic.
            ExitClass::Internal => 101,
        }
    }
}

/// An error any cuno crate can return to `cuno-cli`, which reports it in one
/// place, `error: <Display>` then `help: <help>` when there is one, and
/// returns [`ExitClass::code`] for the binary to exit with
/// (`crates/cuno-cli/src/lib.rs`, `report`).
pub trait CunoError: std::error::Error {
    /// Which exit code this error leads to.
    fn exit_class(&self) -> ExitClass;

    /// How to fix the problem, shown on a `help:` line.
    fn help(&self) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mutation: change any code in `ExitClass::code`; this test fails.
    #[test]
    fn exit_codes_match_the_spec() {
        assert_eq!(ExitClass::Failure.code(), 1);
        assert_eq!(ExitClass::Usage.code(), 2);
        assert_eq!(ExitClass::Setup.code(), 3);
        assert_eq!(ExitClass::Internal.code(), 101);
    }

    #[derive(Debug)]
    struct NoHelp;
    impl std::fmt::Display for NoHelp {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("x")
        }
    }
    impl std::error::Error for NoHelp {}
    impl CunoError for NoHelp {
        fn exit_class(&self) -> ExitClass {
            ExitClass::Failure
        }
    }

    /// Mutation: make the default `help` return `Some(..)`; every error
    /// without its own help would then print an empty `help:` line, and this
    /// test fails.
    #[test]
    fn errors_have_no_help_unless_they_give_one() {
        assert_eq!(NoHelp.help(), None);
    }
}
