//! The operating system cuno runs on, received as a value so that every
//! platform's branch is tested on any machine (`docs/en/specs/architecture.md`,
//! "Dependencies between crates").

use cuno_core::NameCase;

/// The host operating system, as far as finding tools cares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostOs {
    /// Windows: executables end in `.exe`, and LLVM and Visual Studio have
    /// standard install locations to look in.
    Windows,
    /// Any other system.
    Other,
}

impl HostOs {
    /// The system cuno itself was built for. Only the binary calls this.
    pub fn current() -> HostOs {
        if cfg!(windows) {
            HostOs::Windows
        } else {
            HostOs::Other
        }
    }

    /// How this system's file systems compare names: on Windows, without
    /// regard to case.
    pub fn name_case(self) -> NameCase {
        match self {
            HostOs::Windows => NameCase::Insensitive,
            HostOs::Other => NameCase::Sensitive,
        }
    }

    /// `name` as an executable file name on this system.
    pub fn executable(self, name: &str) -> String {
        match self {
            HostOs::Windows => format!("{name}.exe"),
            HostOs::Other => name.to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mutation: drop or misplace the `.exe` suffix; this test fails.
    #[test]
    fn executables_are_named_for_the_host() {
        assert_eq!(HostOs::Windows.executable("clang"), "clang.exe");
        assert_eq!(HostOs::Other.executable("clang"), "clang");
    }
}
