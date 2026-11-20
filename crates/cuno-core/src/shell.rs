//! Writing what cuno shows the user: errors, warnings, help and status lines,
//! in colour or not. See `docs/en/specs/commands.md`, "Output".

use std::io::Write;

use crate::{CunoError, Env};

/// The `--color` / `[term] color` choice (`docs/en/specs/commands.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ColorChoice {
    /// Colour only when writing to a terminal and `NO_COLOR` is unset.
    #[default]
    Auto,
    /// Always colour.
    Always,
    /// Never colour.
    Never,
}

const RESET: &str = "\x1b[0m";
const BOLD_RED: &str = "\x1b[1;31m";
const BOLD_YELLOW: &str = "\x1b[1;33m";
const BOLD_CYAN: &str = "\x1b[1;36m";
const BOLD_GREEN: &str = "\x1b[1;32m";

/// Where cuno's messages go (stderr for the binary). Crates write through a
/// `Shell` instead of printing, so the colour decision is made once.
pub struct Shell {
    out: Box<dyn Write>,
    color: bool,
}

impl Shell {
    /// A shell writing to `out`. `out_is_terminal` says whether `out` is a
    /// terminal, which `ColorChoice::Auto` needs.
    ///
    /// `NO_COLOR` (any value) turns `Auto` into no colour, but not `Always`:
    /// an explicit choice wins, as `commands.md` specifies.
    pub fn new(
        out: Box<dyn Write>,
        choice: ColorChoice,
        env: &Env,
        out_is_terminal: bool,
    ) -> Shell {
        let color = match choice {
            ColorChoice::Always => true,
            ColorChoice::Never => false,
            ColorChoice::Auto => out_is_terminal && env.get("NO_COLOR").is_none(),
        };
        Shell { out, color }
    }

    /// Whether this shell writes colour.
    pub fn color(&self) -> bool {
        self.color
    }

    /// `error: <error>`, then `help: <help>` when the error has one.
    pub fn error(&mut self, error: &dyn CunoError) {
        self.line(BOLD_RED, "error", &error.to_string());
        if let Some(help) = error.help() {
            self.line(BOLD_CYAN, "help", &help);
        }
    }

    /// `warning: <message>`.
    pub fn warning(&mut self, message: &str) {
        self.line(BOLD_YELLOW, "warning", message);
    }

    /// A status line: `verb` right-aligned in 12 columns, then `message`, as
    /// in `   Compiling mygame v0.1.0`.
    pub fn status(&mut self, verb: &str, message: &str) {
        let verb = format!("{verb:>12}");
        let verb = self.paint(BOLD_GREEN, &verb);
        // A failed write to stderr cannot be reported anywhere better.
        let _ = writeln!(self.out, "{verb} {message}");
    }

    fn line(&mut self, style: &str, label: &str, message: &str) {
        let label = self.paint(style, label);
        let _ = writeln!(self.out, "{label}: {message}");
    }

    fn paint(&self, style: &str, text: &str) -> String {
        if self.color {
            format!("{style}{text}{RESET}")
        } else {
            text.to_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ExitClass;
    use std::sync::{Arc, Mutex};

    /// A writer the test can read back after the `Shell` has written to it.
    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl Write for Captured {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().write(buf)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Captured {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    fn shell(choice: ColorChoice, env: &[(&str, &str)], terminal: bool) -> (Shell, Captured) {
        let out = Captured::default();
        let env = Env::from_pairs(env.iter().map(|(n, v)| (n.into(), v.into())));
        (
            Shell::new(Box::new(out.clone()), choice, &env, terminal),
            out,
        )
    }

    #[derive(Debug)]
    struct Fake(Option<&'static str>);
    impl std::fmt::Display for Fake {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("no compiler found")
        }
    }
    impl std::error::Error for Fake {}
    impl CunoError for Fake {
        fn exit_class(&self) -> ExitClass {
            ExitClass::Config
        }
        fn help(&self) -> Option<String> {
            self.0.map(str::to_owned)
        }
    }

    /// Mutation: drop the `help:` line from `Shell::error`, or change a
    /// label; this test fails.
    #[test]
    fn errors_print_error_then_help() {
        let (mut sh, out) = shell(ColorChoice::Never, &[], false);
        sh.error(&Fake(Some("install Build Tools")));
        sh.error(&Fake(None));
        assert_eq!(
            out.text(),
            "error: no compiler found\nhelp: install Build Tools\nerror: no compiler found\n"
        );
    }

    /// Mutation: change the width in `Shell::status`; this test fails.
    #[test]
    fn status_verbs_are_right_aligned_in_twelve_columns() {
        let (mut sh, out) = shell(ColorChoice::Never, &[], false);
        sh.status("Compiling", "mygame v0.1.0");
        sh.warning("unused key");
        assert_eq!(
            out.text(),
            "   Compiling mygame v0.1.0\nwarning: unused key\n"
        );
    }

    /// Mutation: ignore `NO_COLOR`, let it override `Always`, or colour when
    /// not a terminal under `Auto`; this test fails.
    #[test]
    fn color_follows_the_choice_the_terminal_and_no_color() {
        let on = |choice, env: &[(&str, &str)], terminal| shell(choice, env, terminal).0.color();
        assert!(on(ColorChoice::Auto, &[], true));
        assert!(!on(ColorChoice::Auto, &[], false));
        assert!(!on(ColorChoice::Auto, &[("NO_COLOR", "1")], true));
        assert!(on(ColorChoice::Always, &[("NO_COLOR", "1")], false));
        assert!(!on(ColorChoice::Never, &[], true));
    }

    /// Mutation: stop painting the label when colour is on; this test fails.
    #[test]
    fn colour_wraps_the_label_only() {
        let (mut sh, out) = shell(ColorChoice::Always, &[], false);
        sh.warning("x");
        assert_eq!(out.text(), "\x1b[1;33mwarning\x1b[0m: x\n");
    }
}
