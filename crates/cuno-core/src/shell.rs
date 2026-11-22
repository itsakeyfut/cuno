//! Writing what cuno shows the user: errors, warnings, help and status lines,
//! in color or not. See `docs/en/specs/commands.md`, "Output".

use std::fmt::Display;
use std::io::Write;

use crate::Env;

/// A color choice given with `--color` or `[term] color`
/// (`docs/en/specs/commands.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorChoice {
    /// Color only when writing to a terminal.
    Auto,
    /// Always color.
    Always,
    /// Never color.
    Never,
}

const RESET: &str = "\x1b[0m";
const BOLD_RED: &str = "\x1b[1;31m";
const BOLD_YELLOW: &str = "\x1b[1;33m";
const BOLD_CYAN: &str = "\x1b[1;36m";
const BOLD_GREEN: &str = "\x1b[1;32m";

/// Where cuno's messages go (stderr for the binary). Crates write through a
/// `Shell` instead of printing, so the color decision is made once.
pub struct Shell {
    out: Box<dyn Write>,
    color: bool,
}

impl Shell {
    /// A shell writing to `out`. `choice` is the color choice the user gave,
    /// or `None` when they gave none; `out_is_terminal` says whether `out` is
    /// a terminal.
    ///
    /// With no choice given, `NO_COLOR` (set to anything, including empty)
    /// means no color, and otherwise color follows the terminal. A choice the
    /// user gave always wins over `NO_COLOR`, including an explicit `auto`, as
    /// `commands.md` specifies.
    pub fn new(
        out: Box<dyn Write>,
        choice: Option<ColorChoice>,
        env: &Env,
        out_is_terminal: bool,
    ) -> Shell {
        let color = match choice {
            Some(ColorChoice::Always) => true,
            Some(ColorChoice::Never) => false,
            Some(ColorChoice::Auto) => out_is_terminal,
            None => out_is_terminal && env.get("NO_COLOR").is_none(),
        };
        Shell { out, color }
    }

    /// Whether this shell writes color escape codes.
    pub fn uses_color(&self) -> bool {
        self.color
    }

    /// `error: <message>`.
    pub fn error(&mut self, message: impl Display) {
        self.line(BOLD_RED, "error", message);
    }

    /// `warning: <message>`.
    pub fn warning(&mut self, message: impl Display) {
        self.line(BOLD_YELLOW, "warning", message);
    }

    /// `help: <message>`, how to fix what the previous line reported.
    pub fn help(&mut self, message: impl Display) {
        self.line(BOLD_CYAN, "help", message);
    }

    /// A status line: `verb` right-aligned in 12 columns, then `message`, as
    /// in `   Compiling mygame v0.1.0`.
    pub fn status(&mut self, verb: &str, message: impl Display) {
        let verb = self.paint(BOLD_GREEN, &format!("{verb:>12}"));
        self.write(format_args!("{verb} {message}"));
    }

    /// Another program's output (a compiler's diagnostics), written as it
    /// was produced: not decoded, so text in the console's code page reaches
    /// the console as such, and without anything added.
    pub fn raw(&mut self, bytes: &[u8]) {
        // A failed write to stderr cannot be reported anywhere better.
        let _ = self.out.write_all(bytes);
        let _ = self.out.flush();
    }

    fn line(&mut self, style: &str, label: &str, message: impl Display) {
        let label = self.paint(style, label);
        self.write(format_args!("{label}: {message}"));
    }

    fn write(&mut self, line: std::fmt::Arguments<'_>) {
        // A failed write to stderr cannot be reported anywhere better.
        let _ = writeln!(self.out, "{line}");
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
    use crate::env::NameCase;
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

    fn shell(
        choice: Option<ColorChoice>,
        env: &[(&str, &str)],
        terminal: bool,
    ) -> (Shell, Captured) {
        let out = Captured::default();
        let env = Env::from_pairs(
            env.iter().map(|(n, v)| (n.into(), v.into())),
            NameCase::Sensitive,
        );
        (
            Shell::new(Box::new(out.clone()), choice, &env, terminal),
            out,
        )
    }

    /// Mutation: change a label in `error`, `warning`, `help` or `status`,
    /// or the width in `status`; this test fails.
    #[test]
    fn lines_without_color() {
        let (mut sh, out) = shell(Some(ColorChoice::Never), &[], false);
        sh.error("no compiler found");
        sh.help("install Build Tools");
        sh.warning("unused key");
        sh.status("Compiling", "mygame v0.1.0");
        assert_eq!(
            out.text(),
            "error: no compiler found\nhelp: install Build Tools\nwarning: unused key\n   Compiling mygame v0.1.0\n"
        );
    }

    /// Another program's output goes through byte for byte, with nothing
    /// added and nothing painted, even when it is not UTF-8.
    ///
    /// Mutation: decode it, add a newline, or paint it; this test fails.
    #[test]
    fn raw_output_is_written_as_produced() {
        let (mut sh, out) = shell(Some(ColorChoice::Always), &[], false);
        let cp932 = [0x83, 0x81, 0x83, 0x82, 0x0d, 0x0a];
        sh.raw(&cp932);
        sh.raw(b"x");
        assert_eq!(*out.0.lock().unwrap(), [&cp932[..], b"x"].concat());
    }

    /// Mutation: swap or drop the style of any label, or stop painting the
    /// status verb; this test fails.
    #[test]
    fn lines_with_color_paint_only_the_label() {
        let (mut sh, out) = shell(Some(ColorChoice::Always), &[], false);
        sh.error("e");
        sh.help("h");
        sh.warning("w");
        sh.status("Compiling", "m");
        assert_eq!(
            out.text(),
            "\x1b[1;31merror\x1b[0m: e\n\
             \x1b[1;36mhelp\x1b[0m: h\n\
             \x1b[1;33mwarning\x1b[0m: w\n\
             \x1b[1;32m   Compiling\x1b[0m m\n"
        );
    }

    /// Mutation: let `NO_COLOR` override an explicit choice (including an
    /// explicit `auto`), ignore it when no choice was given, or color when not
    /// a terminal under `auto`; this test fails.
    #[test]
    fn color_follows_the_choice_the_terminal_and_no_color() {
        let on =
            |choice, env: &[(&str, &str)], terminal| shell(choice, env, terminal).0.uses_color();
        let no_color: &[(&str, &str)] = &[("NO_COLOR", "")];
        // No choice given: the terminal decides, and NO_COLOR (even empty)
        // turns color off.
        assert!(on(None, &[], true));
        assert!(!on(None, &[], false));
        assert!(!on(None, no_color, true));
        // A choice given wins over NO_COLOR.
        assert!(on(Some(ColorChoice::Auto), no_color, true));
        assert!(!on(Some(ColorChoice::Auto), &[], false));
        assert!(on(Some(ColorChoice::Always), no_color, false));
        assert!(!on(Some(ColorChoice::Never), &[], true));
    }
}
