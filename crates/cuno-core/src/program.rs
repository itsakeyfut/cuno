//! Starting a program cuno built: what `cuno run` and the test runner both
//! decide the same way (`docs/en/specs/architecture.md`, "What goes in
//! `cuno-core`"). How its standard streams are handled is the caller's:
//! `cuno run` leaves them connected, the test runner captures them.

use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

use crate::Env;

/// The command that runs `exe` with `args`: with exactly the variables of
/// `env`, the environment cuno was given rather than the compiler's, and
/// the project `root` as its working directory (`docs/en/specs/commands.md`,
/// "`cuno run`"; `docs/en/specs/testing.md`, "Execution").
pub fn command(exe: &Path, args: &[OsString], env: &Env, root: &Path) -> Command {
    let mut command = Command::new(exe);
    command
        .args(args)
        .env_clear()
        .envs(env.vars())
        .current_dir(root);
    command
}

/// `path` as cuno shows a program it runs: relative to `cwd` when it lies
/// there, as it is otherwise.
pub fn shown<'a>(path: &'a Path, cwd: &Path) -> &'a Path {
    path.strip_prefix(cwd).unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NameCase;
    use std::ffi::OsStr;

    /// The command runs the program with its arguments in the root, with
    /// the variables given and nothing else.
    ///
    /// Mutation: leave out `env_clear`, the variables, the arguments, or
    /// the working directory, or use another; this test fails.
    #[test]
    fn a_program_runs_in_the_root_with_the_environment_given() {
        let env = Env::from_pairs([("ONLY".into(), "1".into())], NameCase::Insensitive);
        let args = [OsString::from("--exact"), OsString::from("t")];
        let cmd = command(Path::new("C:/p/app.exe"), &args, &env, Path::new("C:/p"));
        assert_eq!(cmd.get_program(), OsStr::new("C:/p/app.exe"));
        assert_eq!(cmd.get_args().collect::<Vec<_>>(), ["--exact", "t"]);
        assert_eq!(cmd.get_current_dir(), Some(Path::new("C:/p")));
        let vars: Vec<_> = cmd.get_envs().collect();
        assert_eq!(vars, [(OsStr::new("ONLY"), Some(OsStr::new("1")))]);
        // `env_clear` leaves no trace in `get_envs`; a program that prints
        // its environment shows it took effect.
        let (shell, list): (OsString, &[&str]) = match std::env::var_os("ComSpec") {
            Some(cmd) => (cmd, &["/C", "set"]),
            None => ("/usr/bin/env".into(), &[]),
        };
        let list: Vec<OsString> = list.iter().map(OsString::from).collect();
        let printed = command(Path::new(&shell), &list, &env, &std::env::temp_dir())
            .output()
            .unwrap()
            .stdout;
        let printed = String::from_utf8_lossy(&printed);
        assert!(printed.contains("ONLY=1"), "{printed}");
        assert!(!printed.contains("CARGO_PKG_NAME="), "{printed}");
    }

    /// A path in the current directory is shown relative to it, and one
    /// elsewhere as it is.
    ///
    /// Mutation: show every path as it is, or strip another prefix; this
    /// test fails.
    #[test]
    fn a_program_is_shown_relative_to_the_current_directory() {
        let cwd = Path::new("C:/p");
        assert_eq!(
            shown(Path::new("C:/p/target/debug/app.exe"), cwd),
            Path::new("target/debug/app.exe")
        );
        assert_eq!(
            shown(Path::new("D:/other/app.exe"), cwd),
            Path::new("D:/other/app.exe")
        );
    }
}
